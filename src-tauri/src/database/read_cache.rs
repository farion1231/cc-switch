//! Small, versioned caches for reads on the proxy hot path. Never cache errors.
//! SQLite update hooks invalidate only the affected tables; usage/health writes
//! do not evict configuration. Backup API replacement explicitly invalidates all.

use super::Database;
use crate::{
    error::AppError,
    provider::Provider,
    proxy::{types::AppProxyConfig, usage::calculator::ModelPricing},
};
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicU64, Ordering},
        RwLock,
    },
};

pub(crate) struct ReadCache<V> {
    generation: AtomicU64,
    values: RwLock<HashMap<String, (u64, V)>>,
}

impl<V> Default for ReadCache<V> {
    fn default() -> Self {
        Self {
            generation: AtomicU64::new(0),
            values: RwLock::new(HashMap::new()),
        }
    }
}

impl<V: Clone> ReadCache<V> {
    pub(super) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    pub(crate) fn get(&self, key: &str) -> Option<V> {
        let generation = self.generation.load(Ordering::Acquire);
        let values = self.values.read().ok()?;
        values
            .get(key)
            .filter(|(version, _)| *version == generation)
            .map(|(_, value)| value.clone())
    }

    pub(crate) fn get_or_load(
        &self,
        key: &str,
        load: impl FnOnce() -> Result<V, AppError>,
    ) -> Result<V, AppError> {
        if let Some(value) = self.get(key) {
            return Ok(value);
        }
        // Stamp BEFORE loading. A concurrent write/restore must never label an
        // old query result with the new generation. No cache lock is held during IO.
        let generation = self.generation.load(Ordering::Acquire);
        let value = load()?;
        if generation == self.generation.load(Ordering::Acquire) {
            let mut values = self.values.write().unwrap_or_else(|e| e.into_inner());
            values.retain(|_, (version, _)| *version == generation);
            // Model/provider names may come from requests. Bound arbitrary keys.
            if values.len() >= 512 {
                values.clear();
            }
            values.insert(key.to_owned(), (generation, value.clone()));
        }
        Ok(value)
    }
}

#[derive(Default)]
pub(crate) struct ProxyReadCache {
    pub proxy: ReadCache<AppProxyConfig>,
    pub pricing_source: ReadCache<String>,
    pub settings: ReadCache<Option<String>>,
    pub pricing: ReadCache<Option<ModelPricing>>,
    pub providers: ReadCache<Option<Provider>>,
    pub provider_exists: ReadCache<bool>,
    pub current_provider: ReadCache<Option<String>>,
    pub failover: ReadCache<Vec<Provider>>,
}

impl ProxyReadCache {
    pub(super) fn invalidate_table(&self, table: &str) {
        match table {
            "proxy_config" => {
                self.proxy.invalidate();
                self.pricing_source.invalidate();
            }
            "settings" => self.settings.invalidate(),
            "model_pricing" => self.pricing.invalidate(),
            "providers" | "provider_endpoints" => {
                self.providers.invalidate();
                self.provider_exists.invalidate();
                self.current_provider.invalidate();
                self.failover.invalidate();
            }
            _ => {}
        }
    }

    pub(super) fn invalidate_all(&self) {
        for table in ["proxy_config", "settings", "model_pricing", "providers"] {
            self.invalidate_table(table);
        }
    }
}

impl Database {
    pub(crate) fn cached_model_pricing(
        &self,
        model: &str,
    ) -> Result<Option<ModelPricing>, AppError> {
        self.read_cache.pricing.get_or_load(model, || {
            let conn = super::lock_conn!(self.conn);
            crate::services::usage_stats::find_model_pricing_row(&conn, model)?
                .map(|(input, output, cache_read, cache_creation)| {
                    ModelPricing::from_strings(&input, &output, &cache_read, &cache_creation)
                        .map_err(|e| AppError::Database(format!("解析定价数据失败: {e}")))
                })
                .transpose()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::proxy::types::PrivacyFilterConfig;
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    #[test]
    fn invalidation_during_load_cannot_publish_stale_values() {
        let cache = ReadCache::<String>::default();
        assert_eq!(
            cache
                .get_or_load("key", || {
                    cache.invalidate();
                    Ok("old".into())
                })
                .unwrap(),
            "old"
        );
        assert!(cache.get("key").is_none());
        assert_eq!(
            cache.get_or_load("key", || Ok("new".into())).unwrap(),
            "new"
        );
    }

    #[test]
    fn proxy_daos_support_startup_without_a_current_tokio_runtime() {
        let db = Database::memory().unwrap();
        let config = futures::executor::block_on(db.get_global_proxy_config()).unwrap();
        assert_eq!(config.listen_address, "127.0.0.1");
    }

    #[tokio::test]
    async fn warm_proxy_reads_do_not_wait_for_the_database_mutex() {
        let db = Arc::new(Database::memory().unwrap());
        let provider = Provider::with_id(
            "cached".into(),
            "Cached".into(),
            serde_json::json!({}),
            None,
        );
        db.save_provider(&AppType::Claude, &provider).unwrap();
        db.set_current_provider(&AppType::Claude, "cached").unwrap();
        db.add_to_failover_queue(&AppType::Claude, "cached")
            .unwrap();
        // Warm every read used by routing, switches and billing.
        db.get_proxy_config_for_app(&AppType::Claude).await.unwrap();
        db.get_pricing_model_source(&AppType::Claude).await.unwrap();
        for key in [
            "rectifier_config",
            "optimizer_config",
            "copilot_optimizer_config",
            "privacy_filter_config",
        ] {
            db.get_setting(key).unwrap();
        }
        db.cached_model_pricing("unknown-test-model").unwrap();
        db.provider_exists("cached", &AppType::Claude).unwrap();
        db.get_provider_by_id("cached", &AppType::Claude).unwrap();
        db.get_current_provider(&AppType::Claude).unwrap();
        db.get_failover_providers(&AppType::Claude).unwrap();
        let guard = db.conn.lock().unwrap();
        let worker_db = db.clone();
        let (tx, rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async {
                    worker_db
                        .get_proxy_config_for_app(&AppType::Claude)
                        .await
                        .unwrap();
                    worker_db
                        .get_pricing_model_source(&AppType::Claude)
                        .await
                        .unwrap();
                });
            worker_db.get_rectifier_config().unwrap();
            worker_db.get_optimizer_config().unwrap();
            worker_db.get_copilot_optimizer_config().unwrap();
            worker_db.get_privacy_filter_config().unwrap();
            worker_db
                .cached_model_pricing("unknown-test-model")
                .unwrap();
            assert!(worker_db
                .provider_exists("cached", &AppType::Claude)
                .unwrap());
            worker_db
                .get_provider_by_id("cached", &AppType::Claude)
                .unwrap();
            worker_db.get_current_provider(&AppType::Claude).unwrap();
            worker_db.get_failover_providers(&AppType::Claude).unwrap();
            let _ = tx.send(());
        });
        let completed = rx.recv_timeout(Duration::from_secs(2));
        drop(guard);
        worker.join().unwrap();
        completed.expect("warm proxy reads acquired the SQLite mutex");
    }

    #[tokio::test]
    async fn writes_deletes_and_rollbacks_invalidate_proxy_reads() {
        let db = Database::memory().unwrap();
        let mut config = db.get_proxy_config_for_app(&AppType::Claude).await.unwrap();
        config.max_retries = 2;
        db.update_proxy_config_for_app(config).await.unwrap();
        assert_eq!(
            db.get_proxy_config_for_app(&AppType::Claude)
                .await
                .unwrap()
                .max_retries,
            2
        );
        assert_eq!(
            db.get_pricing_model_source(&AppType::Claude).await.unwrap(),
            "response"
        );
        db.set_pricing_model_source(&AppType::Claude, "request")
            .await
            .unwrap();
        assert_eq!(
            db.get_pricing_model_source(&AppType::Claude).await.unwrap(),
            "request"
        );
        assert!(!db.get_privacy_filter_config().unwrap().enabled);
        db.set_privacy_filter_config(&PrivacyFilterConfig {
            enabled: true,
            filter_tool_results: true,
        })
        .unwrap();
        assert!(db.get_privacy_filter_config().unwrap().enabled);
        // Reuse a prepared DELETE: invalidation must happen when it executes,
        // not just when SQLite first authorizes/prepares the statement.
        for _ in 0..2 {
            db.set_privacy_filter_config(&PrivacyFilterConfig {
                enabled: true,
                filter_tool_results: true,
            })
            .unwrap();
            assert!(db.get_privacy_filter_config().unwrap().enabled);
            db.conn
                .lock()
                .unwrap()
                .prepare_cached("DELETE FROM settings")
                .unwrap()
                .execute([])
                .unwrap();
            assert!(!db.get_privacy_filter_config().unwrap().enabled);
        }

        assert!(db.cached_model_pricing("cache-fixture").unwrap().is_none());
        db.conn.lock().unwrap().execute("INSERT INTO model_pricing (model_id, display_name, input_cost_per_million, output_cost_per_million) VALUES ('cache-fixture', 'test', '3', '6')", []).unwrap();
        assert_eq!(
            db.cached_model_pricing("cache-fixture")
                .unwrap()
                .unwrap()
                .input_cost_per_million
                .to_string(),
            "3"
        );
        db.conn.lock().unwrap().execute_batch("BEGIN; UPDATE model_pricing SET input_cost_per_million = '9' WHERE model_id = 'cache-fixture';").unwrap();
        assert_eq!(
            db.cached_model_pricing("cache-fixture")
                .unwrap()
                .unwrap()
                .input_cost_per_million
                .to_string(),
            "9"
        );
        db.conn.lock().unwrap().execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            db.cached_model_pricing("cache-fixture")
                .unwrap()
                .unwrap()
                .input_cost_per_million
                .to_string(),
            "3"
        );
        db.conn
            .lock()
            .unwrap()
            .execute("DELETE FROM model_pricing", [])
            .unwrap();
        assert!(db.cached_model_pricing("cache-fixture").unwrap().is_none());

        let mut provider =
            Provider::with_id("cached".into(), "Old".into(), serde_json::json!({}), None);
        assert!(!db.provider_exists("cached", &AppType::Claude).unwrap());
        db.save_provider(&AppType::Claude, &provider).unwrap();
        assert!(db.provider_exists("cached", &AppType::Claude).unwrap());
        assert_eq!(
            db.get_provider_by_id("cached", &AppType::Claude)
                .unwrap()
                .unwrap()
                .name,
            "Old"
        );
        assert!(db
            .get_failover_providers(&AppType::Claude)
            .unwrap()
            .is_empty());
        provider.name = "New".into();
        db.save_provider(&AppType::Claude, &provider).unwrap();
        db.add_to_failover_queue(&AppType::Claude, "cached")
            .unwrap();
        assert_eq!(
            db.get_provider_by_id("cached", &AppType::Claude)
                .unwrap()
                .unwrap()
                .name,
            "New"
        );
        assert_eq!(
            db.get_failover_providers(&AppType::Claude).unwrap().len(),
            1
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn health_write_waits_off_the_async_executor() {
        let db = Database::memory().unwrap();
        db.save_provider(
            &AppType::Claude,
            &Provider::with_id(
                "fixture".into(),
                "Fixture".into(),
                serde_json::json!({}),
                None,
            ),
        )
        .unwrap();
        let worker_db = db.clone();
        let (locked_tx, locked_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let holder = std::thread::spawn(move || {
            let _guard = worker_db.conn.lock().unwrap();
            locked_tx.send(()).unwrap();
            // Timeout prevents a regression from hanging the test forever.
            release_rx.recv_timeout(Duration::from_secs(3)).ok();
        });
        locked_rx.recv().unwrap();
        let start = std::time::Instant::now();
        let heartbeat = async {
            tokio::time::sleep(Duration::from_millis(20)).await;
            let _ = release_tx.send(());
            assert!(
                start.elapsed() < Duration::from_secs(2),
                "SQLite blocked Tokio"
            );
        };
        let (_, result) = tokio::join!(
            heartbeat,
            db.update_provider_health_with_threshold("fixture", &AppType::Claude, true, None, 3)
        );
        holder.join().unwrap();
        result.unwrap();
    }
}
