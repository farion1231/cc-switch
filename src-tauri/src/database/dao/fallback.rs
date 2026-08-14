//! Fallback Chain DAO
//!
//! 管理回退链路配置（fallback_chain_config 表）和 Selector 抑制状态（selector_suppression 表）。
//! 为 oh-my-pi Fallback Chain 架构提供持久化支撑，替换旧 cooldown_manager 的数据层。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use rusqlite::params;
use serde::{Deserialize, Serialize};

/// 回退链路条目（对应 fallback_chain_config 一行）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackChainEntry {
    /// 原始选择器字符串，如 "provider_id/model" 或通配符 "provider_id/*"
    pub selector_raw: String,
    /// Provider ID（cc-switch 端点级实体的 id）
    pub provider_id: String,
    /// 模型 ID（"*" 表示任意模型）
    pub model_id: String,
}

/// proxy_config 中 fallback 相关配置列
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FallbackProxyConfig {
    /// 是否启用 fallback chain（false = 走旧 failover 路径）
    pub fallback_enabled: bool,
    /// 主模型恢复策略："cooldown-expiry" | "never"
    pub fallback_revert_policy: String,
    /// 重试基础退避（毫秒）
    pub retry_base_delay_ms: u64,
    /// 重试最大退避（毫秒）
    pub retry_max_delay_ms: u64,
}

impl Default for FallbackProxyConfig {
    fn default() -> Self {
        Self {
            fallback_enabled: false,
            fallback_revert_policy: "cooldown-expiry".to_string(),
            retry_base_delay_ms: 500,
            retry_max_delay_ms: 8000,
        }
    }
}

/// Selector 抑制状态行（对应 selector_suppression 表）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SelectorSuppressionRow {
    /// 选择器标识（"provider_id:model_id"）
    pub selector_identity: String,
    pub app_type: String,
    /// 抑制过期时间（ISO8601）
    pub suppressed_until: String,
    /// 连续抑制次数（用于渐进式抑制）
    pub consecutive_count: u32,
}

impl Database {
    // ---- 回退链路配置 ----

    /// 获取指定 app + chain_key 的回退链路（按 selector_index 升序）
    pub fn get_fallback_chain(
        &self,
        app_type: &str,
        chain_key: &str,
    ) -> Result<Vec<FallbackChainEntry>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT selector_raw, provider_id, model_id
                 FROM fallback_chain_config
                 WHERE app_type = ?1 AND chain_key = ?2
                 ORDER BY selector_index ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let entries = stmt
            .query_map(params![app_type, chain_key], |row| {
                Ok(FallbackChainEntry {
                    selector_raw: row.get(0)?,
                    provider_id: row.get(1)?,
                    model_id: row.get(2)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(entries)
    }

    /// 获取指定 app 的全部回退链路（chain_key → 有序条目）
    pub fn get_all_fallback_chains(
        &self,
        app_type: &str,
    ) -> Result<std::collections::HashMap<String, Vec<FallbackChainEntry>>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT chain_key, selector_raw, provider_id, model_id
                 FROM fallback_chain_config
                 WHERE app_type = ?1
                 ORDER BY chain_key ASC, selector_index ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let mut chains: std::collections::HashMap<String, Vec<FallbackChainEntry>> =
            std::collections::HashMap::new();
        let rows = stmt
            .query_map(params![app_type], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    FallbackChainEntry {
                        selector_raw: row.get(1)?,
                        provider_id: row.get(2)?,
                        model_id: row.get(3)?,
                    },
                ))
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        for (key, entry) in rows {
            chains.entry(key).or_default().push(entry);
        }

        Ok(chains)
    }

    /// 覆盖写入一条回退链路（先删后插，事务内完成）
    pub fn save_fallback_chain(
        &self,
        app_type: &str,
        chain_key: &str,
        entries: &[FallbackChainEntry],
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "DELETE FROM fallback_chain_config WHERE app_type = ?1 AND chain_key = ?2",
            params![app_type, chain_key],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        for (index, entry) in entries.iter().enumerate() {
            tx.execute(
                "INSERT INTO fallback_chain_config
                    (app_type, chain_key, selector_index, selector_raw, provider_id, model_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    app_type,
                    chain_key,
                    index as i64,
                    entry.selector_raw,
                    entry.provider_id,
                    entry.model_id
                ],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交回退链路写入失败: {e}")))
    }

    /// 删除一条回退链路
    pub fn delete_fallback_chain(
        &self,
        app_type: &str,
        chain_key: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM fallback_chain_config WHERE app_type = ?1 AND chain_key = ?2",
            params![app_type, chain_key],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 列出指定 app 已定义的全部 chain_key（去重）
    pub fn list_fallback_chain_keys(&self, app_type: &str) -> Result<Vec<String>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT DISTINCT chain_key FROM fallback_chain_config
                 WHERE app_type = ?1 ORDER BY chain_key ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let keys = stmt
            .query_map(params![app_type], |row| row.get::<_, String>(0))
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(keys)
    }

    /// 从故障转移队列重建 `chain_key='default'` 的回退链路。
    ///
    /// 旧 failover 命令（add/remove_from_failover_queue）修改队列后调用，
    /// 确保 default 链路与队列保持一致（fallback 与旧路径共用同一份候选数据）。
    pub fn sync_default_chain_from_queue(&self, app_type: &str) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "DELETE FROM fallback_chain_config WHERE app_type = ?1 AND chain_key = 'default'",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "INSERT INTO fallback_chain_config
                (app_type, chain_key, selector_index, selector_raw, provider_id, model_id)
             SELECT ?1, 'default',
                    ROW_NUMBER() OVER (
                        PARTITION BY app_type
                        ORDER BY COALESCE(sort_index, 2147483647), id ASC
                    ) - 1,
                    id, id, '*'
             FROM providers
             WHERE app_type = ?1 AND in_failover_queue = 1",
            params![app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        tx.commit()
            .map_err(|e| AppError::Database(format!("提交 default 链路重建失败: {e}")))
    }

    // ---- fallback 配置列（proxy_config） ----

    /// 获取 proxy_config 中的 fallback 配置
    pub fn get_fallback_proxy_config(&self, app_type: &str) -> Result<FallbackProxyConfig, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT fallback_enabled, fallback_revert_policy, retry_base_delay_ms, retry_max_delay_ms
                 FROM proxy_config WHERE app_type = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let config = stmt
            .query_row(params![app_type], |row| {
                Ok(FallbackProxyConfig {
                    fallback_enabled: row.get::<_, i64>(0)? != 0,
                    fallback_revert_policy: row.get(1)?,
                    retry_base_delay_ms: row.get::<_, i64>(2)? as u64,
                    retry_max_delay_ms: row.get::<_, i64>(3)? as u64,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(config)
    }

    /// 更新 proxy_config 中的 fallback 配置
    pub fn set_fallback_proxy_config(
        &self,
        app_type: &str,
        config: &FallbackProxyConfig,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "UPDATE proxy_config
             SET fallback_enabled = ?1,
                 fallback_revert_policy = ?2,
                 retry_base_delay_ms = ?3,
                 retry_max_delay_ms = ?4,
                 updated_at = datetime('now')
             WHERE app_type = ?5",
            params![
                config.fallback_enabled as i64,
                config.fallback_revert_policy,
                config.retry_base_delay_ms as i64,
                config.retry_max_delay_ms as i64,
                app_type
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    // ---- Selector 抑制状态 ----

    /// 获取指定 app 的全部抑制状态
    pub fn get_selector_suppressions(&self, app_type: &str) -> Result<Vec<SelectorSuppressionRow>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT selector_identity, app_type, suppressed_until, consecutive_count
                 FROM selector_suppression WHERE app_type = ?1",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let rows = stmt
            .query_map(params![app_type], |row| {
                Ok(SelectorSuppressionRow {
                    selector_identity: row.get(0)?,
                    app_type: row.get(1)?,
                    suppressed_until: row.get(2)?,
                    consecutive_count: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rows)
    }

    /// 写入（覆盖）一条抑制状态
    pub fn upsert_selector_suppression(
        &self,
        row: &SelectorSuppressionRow,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "INSERT OR REPLACE INTO selector_suppression
                (selector_identity, app_type, suppressed_until, consecutive_count)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                row.selector_identity,
                row.app_type,
                row.suppressed_until,
                row.consecutive_count as i64
            ],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 清除一条抑制状态
    pub fn clear_selector_suppression(
        &self,
        app_type: &str,
        selector_identity: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        conn.execute(
            "DELETE FROM selector_suppression WHERE app_type = ?1 AND selector_identity = ?2",
            params![app_type, selector_identity],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    /// 清理过期的抑制状态，返回清除条数
    pub fn cleanup_expired_suppressions(
        &self,
        app_type: &str,
        now_iso: &str,
    ) -> Result<usize, AppError> {
        let conn = lock_conn!(self.conn);
        let removed = conn
            .execute(
                "DELETE FROM selector_suppression WHERE app_type = ?1 AND suppressed_until < ?2",
                params![app_type, now_iso],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(removed)
    }
}
