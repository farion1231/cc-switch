//! 故障转移队列 DAO
//!
//! 管理代理模式下的故障转移队列

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// 故障转移队列条目
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailoverQueueItem {
    pub provider_id: String,
    pub provider_name: String,
    pub queue_order: i32,
    pub enabled: bool,
    pub created_at: i64,
}

impl Database {
    /// 获取故障转移队列（按 queue_order 排序）
    pub fn get_failover_queue(&self, app_type: &str) -> Result<Vec<FailoverQueueItem>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT fq.provider_id, p.name, fq.queue_order, fq.enabled, fq.created_at
                 FROM failover_queue fq
                 JOIN providers p ON fq.provider_id = p.id AND fq.app_type = p.app_type
                 WHERE fq.app_type = ?1
                 ORDER BY fq.queue_order ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let items = stmt
            .query_map([app_type], |row| {
                Ok(FailoverQueueItem {
                    provider_id: row.get(0)?,
                    provider_name: row.get(1)?,
                    queue_order: row.get(2)?,
                    enabled: row.get(3)?,
                    created_at: row.get(4)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(items)
    }

    /// 获取故障转移队列中的供应商（完整 Provider 信息，按顺序）
    pub fn get_failover_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        let queue = self.get_failover_queue(app_type)?;
        let all_providers = self.get_all_providers(app_type)?;

        let mut result = Vec::new();
        for item in queue {
            if item.enabled {
                if let Some(provider) = all_providers.get(&item.provider_id) {
                    result.push(provider.clone());
                }
            }
        }

        Ok(result)
    }

    /// 添加供应商到故障转移队列末尾
    pub fn add_to_failover_queue(&self, app_type: &str, provider_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 获取当前最大 queue_order
        let max_order: i32 = conn
            .query_row(
                "SELECT COALESCE(MAX(queue_order), 0) FROM failover_queue WHERE app_type = ?1",
                [app_type],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64;

        conn.execute(
            "INSERT OR IGNORE INTO failover_queue (app_type, provider_id, queue_order, enabled, created_at)
             VALUES (?1, ?2, ?3, 1, ?4)",
            rusqlite::params![app_type, provider_id, max_order + 1, now],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 从故障转移队列中移除供应商
    pub fn remove_from_failover_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 获取被删除项的 queue_order
        let removed_order: Option<i32> = conn
            .query_row(
                "SELECT queue_order FROM failover_queue WHERE app_type = ?1 AND provider_id = ?2",
                [app_type, provider_id],
                |row| row.get(0),
            )
            .ok();

        // 删除该项
        conn.execute(
            "DELETE FROM failover_queue WHERE app_type = ?1 AND provider_id = ?2",
            [app_type, provider_id],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 重新排序后面的项（填补空隙）
        if let Some(order) = removed_order {
            conn.execute(
                "UPDATE failover_queue
                 SET queue_order = queue_order - 1
                 WHERE app_type = ?1 AND queue_order > ?2",
                rusqlite::params![app_type, order],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(())
    }

    /// 重新排序故障转移队列
    /// provider_ids: 按新顺序排列的 provider_id 列表
    pub fn reorder_failover_queue(
        &self,
        app_type: &str,
        provider_ids: &[String],
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        // 使用事务确保原子性
        conn.execute("BEGIN TRANSACTION", [])
            .map_err(|e| AppError::Database(e.to_string()))?;

        let result = (|| {
            for (index, provider_id) in provider_ids.iter().enumerate() {
                conn.execute(
                    "UPDATE failover_queue
                     SET queue_order = ?3
                     WHERE app_type = ?1 AND provider_id = ?2",
                    rusqlite::params![app_type, provider_id, (index + 1) as i32],
                )
                .map_err(|e| AppError::Database(e.to_string()))?;
            }
            Ok(())
        })();

        match result {
            Ok(_) => {
                conn.execute("COMMIT", [])
                    .map_err(|e| AppError::Database(e.to_string()))?;
                Ok(())
            }
            Err(e) => {
                conn.execute("ROLLBACK", []).ok();
                Err(e)
            }
        }
    }

    /// 设置故障转移队列中供应商的启用状态
    pub fn set_failover_item_enabled(
        &self,
        app_type: &str,
        provider_id: &str,
        enabled: bool,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        let rows_affected = conn
            .execute(
                "UPDATE failover_queue SET enabled = ?3 WHERE app_type = ?1 AND provider_id = ?2",
                rusqlite::params![app_type, provider_id, enabled],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        if rows_affected == 0 {
            log::warn!(
                "set_failover_item_enabled: 未找到匹配记录 app_type={app_type}, provider_id={provider_id}"
            );
            return Err(AppError::Database(format!(
                "未找到故障转移队列项: app_type={app_type}, provider_id={provider_id}"
            )));
        }

        log::info!(
            "set_failover_item_enabled: 已更新 app_type={app_type}, provider_id={provider_id}, enabled={enabled}"
        );

        Ok(())
    }

    /// 清空故障转移队列
    pub fn clear_failover_queue(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute("DELETE FROM failover_queue WHERE app_type = ?1", [app_type])
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 检查供应商是否在故障转移队列中
    pub fn is_in_failover_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);

        let count: i32 = conn
            .query_row(
                "SELECT COUNT(*) FROM failover_queue WHERE app_type = ?1 AND provider_id = ?2",
                [app_type, provider_id],
                |row| row.get(0),
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(count > 0)
    }

    /// 获取可添加到故障转移队列的供应商（不在队列中的）
    pub fn get_available_providers_for_failover(
        &self,
        app_type: &str,
    ) -> Result<Vec<Provider>, AppError> {
        let all_providers = self.get_all_providers(app_type)?;
        let queue = self.get_failover_queue(app_type)?;

        let queue_ids: std::collections::HashSet<_> =
            queue.iter().map(|item| &item.provider_id).collect();

        let available: Vec<Provider> = all_providers
            .into_values()
            .filter(|p| !queue_ids.contains(&p.id))
            .collect();

        Ok(available)
    }
}
