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

/// 分类器队列排序的兜底值：`classifier_sort_index` / `sort_index` 为 NULL 时排到最后
const ORDER_FALLBACK: i64 = 999999;

/// 分类器队列条目（简化版，用于前端展示）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifierQueueItem {
    pub provider_id: String,
    pub provider_name: String,
    pub sort_index: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_notes: Option<String>,
    /// 队列内的独立排序位（NULL = 尚未拖拽过，读取时回落到 `sort_index`）
    pub classifier_sort_index: Option<i64>,
    /// 该条目的出站模型名覆写（None = 透传客户端请求的模型）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl Database {
    /// 获取分类器队列
    ///
    /// 排序键是队列**自己**的 `classifier_sort_index`；为 NULL（v20 之前入队、
    /// 从未拖拽过）时回落到首页的 `sort_index`，所以升级后顺序保持不变。
    pub fn get_classifier_queue(
        &self,
        app_type: &str,
    ) -> Result<Vec<ClassifierQueueItem>, AppError> {
        let conn = lock_conn!(self.conn);
        Self::classifier_queue_on_conn(&conn, app_type)
    }

    fn classifier_queue_on_conn(
        conn: &rusqlite::Connection,
        app_type: &str,
    ) -> Result<Vec<ClassifierQueueItem>, AppError> {
        let mut stmt = conn
            .prepare(
                "SELECT id, name, sort_index, notes, classifier_sort_index, classifier_model
                 FROM providers
                 WHERE app_type = ?1 AND in_classifier_queue = 1
                 ORDER BY COALESCE(classifier_sort_index, ?2),
                          COALESCE(sort_index, ?2),
                          id ASC",
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        let items = stmt
            .query_map(rusqlite::params![app_type, ORDER_FALLBACK], |row| {
                Ok(ClassifierQueueItem {
                    provider_id: row.get(0)?,
                    provider_name: row.get(1)?,
                    sort_index: row.get(2)?,
                    provider_notes: row.get(3)?,
                    classifier_sort_index: row.get(4)?,
                    model: row.get(5)?,
                })
            })
            .map_err(|e| AppError::Database(e.to_string()))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(items)
    }

    /// 添加供应商到分类器队列（追加到队尾）
    ///
    /// 入队后把整条队列的 `classifier_sort_index` 重写成稠密的 0..n，而不是只给
    /// 新成员一个 `max+1`：存量成员的排序位可能全是 NULL，此时 `max` 也是 NULL，
    /// 新成员会拿到 0 并插到队首 —— 与「追加到队尾」正好相反。整条归一化后，
    /// 排序状态在第一次入队时就变得确定，不再依赖 NULL 的回落规则。
    pub fn add_to_classifier_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        tx.execute(
            "UPDATE providers SET in_classifier_queue = 1 WHERE id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 新成员此刻排序位为 NULL，必然落在已有成员之后（COALESCE 兜底值最大），
        // 天然就是队尾；直接按当前读序写回稠密下标即可。
        let ordered: Vec<String> = Self::classifier_queue_on_conn(&tx, app_type)?
            .into_iter()
            .map(|item| item.provider_id)
            .collect();
        Self::write_classifier_order(&tx, app_type, &ordered)?;

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    /// 按给定顺序重排分类器队列（前端拖拽后调用）
    ///
    /// 只更新仍在队列里的成员；`ordered_ids` 里的陌生 id 静默跳过，
    /// 队列里没被点名的成员保留原排序位。这样并发下（另一处刚把某个供应商
    /// 移出队列）不会整体失败，排序仍然确定 —— 未点名者由 ORDER BY 的
    /// `sort_index` / `id` 次级键兜底。
    pub fn reorder_classifier_queue(
        &self,
        app_type: &str,
        ordered_ids: &[String],
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn
            .transaction()
            .map_err(|e| AppError::Database(e.to_string()))?;

        Self::write_classifier_order(&tx, app_type, ordered_ids)?;

        tx.commit().map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    fn write_classifier_order(
        tx: &rusqlite::Transaction<'_>,
        app_type: &str,
        ordered_ids: &[String],
    ) -> Result<(), AppError> {
        for (index, provider_id) in ordered_ids.iter().enumerate() {
            tx.execute(
                "UPDATE providers SET classifier_sort_index = ?1
                 WHERE id = ?2 AND app_type = ?3 AND in_classifier_queue = 1",
                rusqlite::params![index as i64, provider_id, app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;
        }
        Ok(())
    }

    /// 设置队列条目的出站模型名覆写
    ///
    /// 空白字符串等同于清除（`NULL` = 透传客户端模型）。`in_classifier_queue = 1`
    /// 是 WHERE 的一部分：模型覆写只对队列成员有意义，写到非成员身上会变成
    /// 一份看不见、却会在重新入队时突然复活的隐藏配置。
    pub fn set_classifier_model(
        &self,
        app_type: &str,
        provider_id: &str,
        model: Option<&str>,
    ) -> Result<(), AppError> {
        let normalized = model
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string);

        let conn = lock_conn!(self.conn);

        let updated = conn
            .execute(
                "UPDATE providers SET classifier_model = ?1
                 WHERE id = ?2 AND app_type = ?3 AND in_classifier_queue = 1",
                rusqlite::params![normalized, provider_id, app_type],
            )
            .map_err(|e| AppError::Database(e.to_string()))?;

        if updated == 0 {
            return Err(AppError::Database(format!(
                "供应商 {provider_id} 不在 {app_type} 的分类器队列中"
            )));
        }

        Ok(())
    }

    /// 从分类器队列中移除供应商
    ///
    /// 与 `remove_from_failover_queue` 刻意不同：这里**不**删除 provider_health 行。
    /// 同一个供应商可能同时在故障转移队列里，那边的健康状态是活数据，
    /// 退出分类器队列不应该把它清掉。
    ///
    /// 排序位与模型覆写则相反 —— 它们是队列私有的，跟着一起清掉。留着会变成
    /// 界面上看不见的残留配置，在重新入队时带着一个用户早已忘记的模型名复活。
    pub fn remove_from_classifier_queue(
        &self,
        app_type: &str,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE providers
             SET in_classifier_queue = 0, classifier_sort_index = NULL, classifier_model = NULL
             WHERE id = ?1 AND app_type = ?2",
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
            "UPDATE providers
             SET in_classifier_queue = 0, classifier_sort_index = NULL, classifier_model = NULL
             WHERE app_type = ?1",
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
    fn add_appends_to_the_tail_and_densifies_order() {
        let db = Database::memory().expect("memory db");
        // 首页顺序刻意与入队顺序相反，验证队列顺序不再受 sort_index 支配
        for (id, sort_index) in [("a", 3), ("b", 2), ("c", 1)] {
            let mut provider = Provider::with_id(
                id.to_string(),
                format!("provider-{id}"),
                json!({ "auth": {}, "config": "" }),
                None,
            );
            provider.sort_index = Some(sort_index);
            db.save_provider("claude", &provider).expect("save");
        }

        db.add_to_classifier_queue("claude", "a").unwrap();
        db.add_to_classifier_queue("claude", "b").unwrap();
        db.add_to_classifier_queue("claude", "c").unwrap();

        let queue = db.get_classifier_queue("claude").unwrap();
        let ordered: Vec<String> = queue.iter().map(|i| i.provider_id.clone()).collect();
        assert_eq!(ordered, vec!["a", "b", "c"], "先入队的应排在前面");
        assert_eq!(
            queue
                .iter()
                .map(|i| i.classifier_sort_index)
                .collect::<Vec<_>>(),
            vec![Some(0), Some(1), Some(2)],
            "入队后排序位应归一化为稠密的 0..n"
        );
    }

    #[test]
    fn reorder_persists_the_new_order() {
        let db = Database::memory().expect("memory db");
        for id in ["a", "b", "c"] {
            seed(&db, "claude", id);
            db.add_to_classifier_queue("claude", id).unwrap();
        }

        db.reorder_classifier_queue(
            "claude",
            &["c".to_string(), "a".to_string(), "b".to_string()],
        )
        .unwrap();

        let ordered: Vec<String> = db
            .get_classifier_queue("claude")
            .unwrap()
            .into_iter()
            .map(|item| item.provider_id)
            .collect();
        assert_eq!(ordered, vec!["c", "a", "b"]);
    }

    #[test]
    fn reorder_ignores_ids_outside_the_queue() {
        // 前端可能拿着一份刚被别处改过的旧列表；陌生 id 不应让整次重排失败
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");
        seed(&db, "claude", "outsider");
        db.add_to_classifier_queue("claude", "a").unwrap();

        db.reorder_classifier_queue(
            "claude",
            &["outsider".to_string(), "a".to_string(), "ghost".to_string()],
        )
        .unwrap();

        let queue = db.get_classifier_queue("claude").unwrap();
        assert_eq!(queue.len(), 1);
        assert_eq!(queue[0].provider_id, "a");
    }

    #[test]
    fn model_override_roundtrip_and_clearing() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");
        db.add_to_classifier_queue("claude", "a").unwrap();
        assert_eq!(db.get_classifier_queue("claude").unwrap()[0].model, None);

        db.set_classifier_model("claude", "a", Some("  glm-4-flash  "))
            .unwrap();
        assert_eq!(
            db.get_classifier_queue("claude").unwrap()[0]
                .model
                .as_deref(),
            Some("glm-4-flash"),
            "两侧空白应被裁掉"
        );

        // 空白等同于清除
        db.set_classifier_model("claude", "a", Some("   ")).unwrap();
        assert_eq!(db.get_classifier_queue("claude").unwrap()[0].model, None);
    }

    #[test]
    fn model_override_rejects_non_members() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");

        assert!(db.set_classifier_model("claude", "a", Some("m")).is_err());
    }

    #[test]
    fn remove_clears_queue_private_columns() {
        let db = Database::memory().expect("memory db");
        seed(&db, "claude", "a");
        db.add_to_classifier_queue("claude", "a").unwrap();
        db.set_classifier_model("claude", "a", Some("glm-4-flash"))
            .unwrap();

        db.remove_from_classifier_queue("claude", "a").unwrap();
        db.add_to_classifier_queue("claude", "a").unwrap();

        let item = &db.get_classifier_queue("claude").unwrap()[0];
        assert_eq!(item.model, None, "重新入队不应带回旧的模型覆写");
    }

    #[test]
    fn legacy_rows_without_order_fall_back_to_sort_index() {
        // v20 之前入队的行 classifier_sort_index 为 NULL，顺序必须仍按首页排序，
        // 否则升级当下用户会看到队列莫名重排
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
        }
        db.conn
            .lock()
            .unwrap()
            .execute(
                "UPDATE providers SET in_classifier_queue = 1, classifier_sort_index = NULL",
                [],
            )
            .expect("seed legacy queue rows");

        let ordered: Vec<String> = db
            .get_classifier_queue("claude")
            .unwrap()
            .into_iter()
            .map(|item| item.provider_id)
            .collect();
        assert_eq!(ordered, vec!["b", "c", "a"]);
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
