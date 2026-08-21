//! 分类器队列 DAO
//!
//! 管理代理模式下的分类器队列（基于 providers 表的 in_classifier_queue 字段）。
//!
//! 分类器队列服务于 Claude Code Auto Mode 在执行 Bash 命令前发出的「安全分类器」
//! 请求：该请求有客户端硬超时，把它分流到响应快的供应商可避免超时。

use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

/// 分类器队列条目（简化版，用于前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifierQueueItem {
    pub provider_id: String,
    pub provider_name: String,
    pub sort_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_notes: Option<String>,
}

impl Database {
    /// 获取分类器队列（按 sort_index 排序，与故障转移队列同源于首页拖拽顺序）
    pub fn get_classifier_queue(
        &self,
        app_type: &str,
    ) -> Result<Vec<ClassifierQueueItem>, AppError> {
        let conn = lock_conn!(self.conn);

        let mut stmt = conn
            .prepare(
                "SELECT id, name, sort_index, notes
                 FROM providers
                 WHERE app_type = ?1 AND in_classifier_queue = 1
                 ORDER BY COALESCE(sort_index, 999999), id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let items = stmt
            .query_map([app_type], |row| {
                Ok(ClassifierQueueItem {
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

    /// 添加供应商到分类器队列
    pub fn add_to_classifier_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE providers SET in_classifier_queue = 1 WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 从分类器队列中移除供应商
    ///
    /// 与 `remove_from_failover_queue` 刻意不同：这里**不**删除 provider_health 行。
    /// 同一个供应商可能同时在故障转移队列里，那边的健康状态是活数据，
    /// 退出分类器队列不应该把它清掉。
    pub fn remove_from_classifier_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE providers SET in_classifier_queue = 0 WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        log::info!("已从分类器队列移除供应商 {provider_id} ({app_type})");

        Ok(())
    }

    /// 清空分类器队列
    #[allow(dead_code)]
    pub fn clear_classifier_queue(&self, app_type: &str) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE providers SET in_classifier_queue = 0 WHERE app_type = ?1",
            [app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 检查供应商是否在分类器队列中
    pub fn is_in_classifier_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<bool, AppError> {
        let conn = lock_conn!(self.conn);

        let in_queue: bool = conn
            .query_row(
                "SELECT in_classifier_queue FROM providers WHERE id = ?1 AND app_type = ?2",
                rusqlite::params![provider_id, app_type],
                |row| row.get(0),
            )
            .unwrap_or(false);

        Ok(in_queue)
    }

    /// 获取可添加到分类器队列的供应商（不在队列中的）
    ///
    /// 刻意走「队列 id 集合做差集」而不是 `Provider` 上的布尔字段：后者会逼着
    /// `Provider` 结构体加字段，进而牵动 dao/providers.rs 里多处 SELECT/INSERT 列清单。
    pub fn get_available_providers_for_classifier(
        &self,
        app_type: &str,
    ) -> Result<Vec<Provider>, AppError> {
        let queued: HashSet<String> = self
            .get_classifier_queue(app_type)?
            .into_iter()
            .map(|item| item.provider_id)
            .collect();

        Ok(self
            .get_all_providers(app_type)?
            .into_values()
            .filter(|p| !queued.contains(&p.id))
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use crate::database::Database;
    use crate::provider::Provider;
    use serde_json::json;

    fn seed(db: &Database, app_type: &str, id: &str) {
        let provider = Provider::with_id(
            id.to_string(),
            format!("provider-{id}"),
            json!({ "auth": {}, "config": "" }),
            None,
        );
        db.save_provider(app_type, &provider).expect("save");
    }

    #[test]
    fn add_remove_roundtrip() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");

        assert!(!db.is_in_classifier_queue("claude", "a").unwrap());
        db.add_to_classifier_queue("claude", "a").unwrap();
        assert!(db.is_in_classifier_queue("claude", "a").unwrap());
        assert_eq!(db.get_classifier_queue("claude").unwrap().len(), 1);

        db.remove_from_classifier_queue("claude", "a").unwrap();
        assert!(!db.is_in_classifier_queue("claude", "a").unwrap());
        assert!(db.get_classifier_queue("claude").unwrap().is_empty());
    }

    #[test]
    fn queue_is_scoped_per_app_type() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");
        seed(&db, "codex", "a");

        db.add_to_classifier_queue("claude", "a").unwrap();

        assert_eq!(db.get_classifier_queue("claude").unwrap().len(), 1);
        assert!(db.get_classifier_queue("codex").unwrap().is_empty());
    }

    #[test]
    fn queue_orders_by_sort_index() {
        let db = Database::memory().expect("memory db");
        for (id, sort_index) in [("a", 3), ("b", 1), ("c", 2)] {
            let mut provider = Provider::with_id(
                id.to_string(),
                format!("provider-{id}"),
                json!({ "auth": {}, "config": "" }),
                None,
            );
            provider.sort_index = Some(sort_index);
            db.save_provider("claude", &provider).expect("save");
            db.add_to_classifier_queue("claude", id).unwrap();
        }

        let ordered: Vec<String> = db
            .get_classifier_queue("claude")
            .unwrap()
            .into_iter()
            .map(|item| item.provider_id)
            .collect();
        assert_eq!(ordered, vec!["b", "c", "a"]);
    }

    #[test]
    fn available_providers_excludes_queued() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");
        seed(&db, "claude", "b");
        db.add_to_classifier_queue("claude", "a").unwrap();

        let available: Vec<String> = db
            .get_available_providers_for_classifier("claude")
            .unwrap()
            .into_iter()
            .map(|p| p.id)
            .collect();
        assert_eq!(available, vec!["b"]);
    }

    #[tokio::test]
    async fn remove_does_not_delete_provider_health() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");
        db.add_to_classifier_queue("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();
        db.update_provider_health("a", "claude", false, Some("boom".to_string()))
            .await
            .unwrap();
        assert_eq!(
            db.get_provider_health("a", "claude")
                .await
                .unwrap()
                .consecutive_failures,
            1
        );

        db.remove_from_classifier_queue("claude", "a").unwrap();

        // 该供应商还在故障转移队列里，健康行必须保留（行被删掉时会回落成 0）
        assert_eq!(
            db.get_provider_health("a", "claude")
                .await
                .unwrap()
                .consecutive_failures,
            1
        );
    }

    #[test]
    fn provider_can_be_in_both_queues() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");

        db.add_to_failover_queue("claude", "a").unwrap();
        db.add_to_classifier_queue("claude", "a").unwrap();

        assert!(db.is_in_failover_queue("claude", "a").unwrap());
        assert!(db.is_in_classifier_queue("claude", "a").unwrap());

        db.remove_from_classifier_queue("claude", "a").unwrap();
        assert!(db.is_in_failover_queue("claude", "a").unwrap());
    }
}
