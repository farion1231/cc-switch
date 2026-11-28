//! 故障转移队列 DAO
//!
//! 管理代理模式下的故障转移队列（基于 providers 表的 in_failover_queue 字段）

use crate::app_config::AppType;
use crate::database::{lock_conn, Database};
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::provider_router::provider_supports_failover;
use rusqlite::{params, Connection};
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

pub(super) fn has_eligible_failover_provider(
    conn: &Connection,
    app: &AppType,
    exclude: Option<&str>,
) -> Result<bool, AppError> {
    use super::providers::{row_to_provider, PROVIDER_SELECT};
    let mut query = conn.prepare_cached(&format!(
        "{PROVIDER_SELECT} WHERE app_type = ?1 AND in_failover_queue = 1 AND id IS NOT ?2"
    ))?;
    let mut rows = query.query(params![app, exclude])?;
    while let Some(row) = rows.next()? {
        if super::read_row(row, row_to_provider)?
            .is_ok_and(|provider| provider_supports_failover(app, &provider))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Called with the same SQLite guard as the mutation. Old invalid queues can
/// still be edited; a valid enabled queue must retain an eligible member.
pub(super) fn guard_failover_queue_removal(
    conn: &Connection,
    app: &AppType,
    provider_id: &str,
) -> Result<(), AppError> {
    if !app.spec().local_proxy
        || !conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM proxy_config WHERE app_type = ?1 AND auto_failover_enabled != 0)
                AND EXISTS(SELECT 1 FROM providers WHERE app_type = ?1 AND id = ?2 AND in_failover_queue = 1)",
            params![app, provider_id],
            |row| row.get::<_, bool>(0),
        )?
        || has_eligible_failover_provider(conn, app, Some(provider_id))?
        || !has_eligible_failover_provider(conn, app, None)?
    {
        return Ok(());
    }
    Err(AppError::localized(
        "failover.queue.last_provider",
        "自动故障转移已开启。请先加入另一个可用供应商，或关闭自动故障转移。",
        "Automatic failover is enabled. Add another eligible provider or turn off automatic failover first.",
    ))
}

impl Database {
    /// 获取故障转移队列（按 sort_index 排序）
    pub fn get_failover_queue(
        &self,
        app_type: &AppType,
    ) -> Result<Vec<FailoverQueueItem>, AppError> {
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
    pub fn get_failover_providers(&self, app_type: &AppType) -> Result<Vec<Provider>, AppError> {
        self.read_cache.failover.get_or_load(app_type.as_str(), || {
            self.get_failover_providers_uncached(app_type)
        })
    }

    fn get_failover_providers_uncached(
        &self,
        app_type: &AppType,
    ) -> Result<Vec<Provider>, AppError> {
        // 路由不写库：一个成员读不出来就跳过它，用其余成员照常转发。
        Ok(self
            .load_providers(app_type, true)?
            .into_readable("故障转移队列")
            .into_values()
            .collect())
    }

    /// 添加供应商到故障转移队列
    pub fn add_to_failover_queue(
        &self,
        app_type: &AppType,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);

        conn.execute(
            "UPDATE providers SET in_failover_queue = 1 WHERE id = ?1 AND app_type = ?2 AND in_failover_queue = 0",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    pub(crate) fn check_failover_queue_removal(
        &self,
        app: &AppType,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let conn = lock_conn!(self.conn);
        guard_failover_queue_removal(&conn, app, provider_id)
    }

    /// 从故障转移队列中移除供应商
    pub fn remove_from_failover_queue(
        &self,
        app_type: &AppType,
        provider_id: &str,
    ) -> Result<(), AppError> {
        let mut conn = lock_conn!(self.conn);
        let tx = conn.transaction()?;
        guard_failover_queue_removal(&tx, app_type, provider_id)?;

        // 1. 从队列中移除
        tx.execute(
            "UPDATE providers SET in_failover_queue = 0 WHERE id = ?1 AND app_type = ?2 AND in_failover_queue = 1",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;

        // 2. 清除该供应商的健康状态（退出队列后不再需要健康监控）
        tx.execute(
            "DELETE FROM provider_health WHERE provider_id = ?1 AND app_type = ?2",
            rusqlite::params![provider_id, app_type],
        )
        .map_err(|e| AppError::Database(e.to_string()))?;
        tx.commit()?;

        log::info!("已从故障转移队列移除供应商 {provider_id} ({app_type}), 并清除其健康状态");

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn enabled_failover_keeps_an_eligible_member_when_removing_or_deleting() {
        let db = Database::memory().unwrap();
        let app = &AppType::Claude;
        for id in ["a", "b"] {
            let provider = Provider::with_id(id.into(), id.into(), json!({}), None);
            db.save_provider(app, &provider).unwrap();
        }
        let mut official = Provider::with_id("official".into(), "Official".into(), json!({}), None);
        official.category = Some("official".into());
        official.in_failover_queue = true;
        db.save_provider(app, &official).unwrap();
        assert!(db.set_auto_failover_enabled_sync(app, true).is_err());
        db.add_to_failover_queue(app, "a").unwrap();
        db.set_auto_failover_enabled_sync(app, true).unwrap();

        assert!(db.remove_from_failover_queue(app, "a").is_err());
        assert!(db.delete_provider(app, "a").is_err());
        assert!(db
            .get_failover_queue(app)
            .unwrap()
            .iter()
            .any(|p| p.provider_id == "a"));

        db.add_to_failover_queue(app, "b").unwrap();
        db.delete_provider(app, "a").unwrap();
        assert!(db
            .get_failover_queue(app)
            .unwrap()
            .iter()
            .all(|p| p.provider_id != "a"));
        db.set_auto_failover_enabled_sync(app, false).unwrap();
        db.remove_from_failover_queue(app, "b").unwrap();
        assert!(db
            .get_failover_queue(app)
            .unwrap()
            .iter()
            .all(|p| p.provider_id != "b"));
    }

    #[test]
    fn provider_loading_preserves_list_and_queue_order_with_app_local_endpoints() {
        let db = Database::memory().unwrap();
        for (id, created_at, queued) in [("b", 1, true), ("a", 2, true), ("outside", 0, false)] {
            let mut provider =
                Provider::with_id(id.to_string(), id.to_string(), json!({"model": id}), None);
            provider.sort_index = Some(1);
            provider.created_at = Some(created_at);
            provider.in_failover_queue = queued;
            db.save_provider(&AppType::Claude, &provider).unwrap();
            db.add_custom_endpoint(&AppType::Claude, id, &format!("https://{id}.example.com"))
                .unwrap();
        }
        let other_app = Provider::with_id("a".into(), "other app".into(), json!({}), None);
        db.save_provider(&AppType::Codex, &other_app).unwrap();
        db.add_custom_endpoint(&AppType::Codex, "a", "https://codex.example.com")
            .unwrap();

        let all = db.get_all_providers(&AppType::Claude).unwrap();
        assert_eq!(
            all.keys().map(String::as_str).collect::<Vec<_>>(),
            ["outside", "b", "a"]
        );
        let queued = db.get_failover_providers(&AppType::Claude).unwrap();
        let queue = db.get_failover_queue(&AppType::Claude).unwrap();
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
