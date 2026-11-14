//! 故障转移队列 DAO
//!
//! 管理代理模式下的故障转移队列（基于 providers 表的 in_failover_queue 字段）

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};

/// 故障转移队列条目（简化版，用于前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FailoverQueueItem {
    pub provider_id: String,
    pub provider_name: String,
    pub sort_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_notes: Option<String>,
}

impl Database {
    /// 获取故障转移队列（按 sort_index 排序）
    pub fn get_failover_queue(&self, app_type: &str) -> Result<Vec<FailoverQueueItem>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT id, name, sort_index, notes
                 FROM providers
                 WHERE app_type = ?1 AND in_failover_queue = 1
                 ORDER BY COALESCE(sort_index, 999999), id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let items = stmt
            .query_map([app_type], |row| {
                Ok(FailoverQueueItem {
                    provider_id: row.get(0)?,
                    provider_name: row.get(1)?,
                    sort_index: row.get(2)?,
                    provider_notes: row.get(3)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(items)
    }

    /// 获取故障转移队列中的供应商（完整 Provider 信息，与队列的排序一致）
    pub fn get_failover_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        Ok(self.load_providers(app_type, true)?.into_values().collect())
    }

    /// 添加供应商到故障转移队列
    pub fn add_to_failover_queue(&self, app_type: &str, provider_id: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE providers SET in_failover_queue = 1 WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
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

        // 1. 从队列中移除
        conn.execute(
            "UPDATE providers SET in_failover_queue = 0 WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 2. 清除该供应商的健康状态（退出队列后不再需要健康监控）
        conn.execute(
            "DELETE FROM provider_health WHERE provider_id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::info!("已从故障转移队列移除供应商 {provider_id} ({app_type}), 并清除其健康状态");

        Ok(())
    }

    /// 清空故障转移队列
    pub fn clear_failover_queue(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE providers SET in_failover_queue = 0 WHERE app_type = ?1",
            [app_type],
        )
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

        let in_queue: bool = conn
            .query_row(
                "SELECT in_failover_queue FROM providers WHERE id = ?1 AND app_type = ?2",
                rusqlite::params![provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(false);

        Ok(in_queue)
    }

    /// 获取可添加到故障转移队列的供应商（不在队列中的）
    pub fn get_available_providers_for_failover(
        &self,
        app_type: &str,
    ) -> Result<Vec<Provider>, AppError> {
        let all_providers = self.get_all_providers(app_type)?;

        let available: Vec<Provider> = all_providers
            .into_values()
            .filter(|p| !p.in_failover_queue)
            .collect();

        Ok(available)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn provider_loading_preserves_list_and_queue_order_with_app_local_endpoints() {
        let db = Database::memory().unwrap();
        for (id, created_at, queued) in [("b", 1, true), ("a", 2, true), ("outside", 0, false)] {
            let mut provider =
                Provider::with_id(id.to_string(), id.to_string(), json!({"model": id}), None);
            provider.sort_index = Some(1);
            provider.created_at = Some(created_at);
            provider.in_failover_queue = queued;
            db.save_provider("claude", &provider).unwrap();
            db.add_custom_endpoint("claude", id, &format!("https://{id}.example.com"))
                .unwrap();
        }
        let other_app = Provider::with_id("a".into(), "other app".into(), json!({}), None);
        db.save_provider("codex", &other_app).unwrap();
        db.add_custom_endpoint("codex", "a", "https://codex.example.com")
            .unwrap();

        let all = db.get_all_providers("claude").unwrap();
        assert_eq!(
            all.keys().map(String::as_str).collect::<Vec<_>>(),
            ["outside", "b", "a"]
        );
        let queued = db.get_failover_providers("claude").unwrap();
        let queue = db.get_failover_queue("claude").unwrap();
        assert_eq!(
            queued.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
            queue
                .iter()
                .map(|p| p.provider_id.as_str())
                .collect::<Vec<_>>()
        );
        assert_eq!(queued[0].id, "a");
        assert_eq!(queued[1].id, "b");
        for provider in &queued {
            let meta = provider.meta.as_ref().unwrap();
            let url = format!("https://{}.example.com", provider.id);
            assert_eq!(meta.custom_endpoints.len(), 1);
            assert_eq!(meta.custom_endpoints[&url].url, url);
            assert!(meta.custom_endpoints[&url].added_at > 0);
            assert_eq!(
                provider.settings_config["model"].as_str(),
                Some(provider.id.as_str())
            );
            assert!(provider.in_failover_queue);
            let listed = &all[&provider.id].meta.as_ref().unwrap().custom_endpoints;
            assert_eq!(listed.len(), 1);
            assert_eq!(listed[&url].url, url);
            assert_eq!(listed[&url].added_at, meta.custom_endpoints[&url].added_at);
        }
    }
}
