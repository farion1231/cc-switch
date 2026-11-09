//! 代理请求路径上的配置读缓存。
//!
//! 每个代理请求都要读应用级代理配置和三项整流 / 优化配置，原来每次都要拿全局连接锁
//! 查库；统计页刷新、会话用量导入持锁时，请求只能排队等。这些配置很少变，缓存起来，
//! 请求路径就不再碰连接锁。
//!
//! 失效靠代际计数：`settings` / `proxy_config` 表有任何写入时，连接的 update hook 让
//! 计数加一；Backup API 整库替换（导入、恢复备份）不经过 hook，由调用方显式
//! [`ConfigCache::invalidate`]。读取方先取代际再查库，查完只在代际没变时才写回，所以
//! 查库期间发生的写入最多让这次结果不进缓存，不会把旧值留下来。

use crate::proxy::types::AppProxyConfig;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

/// 影响缓存内容的表，写入时让缓存失效
pub(crate) fn invalidates_config_cache(table: &str) -> bool {
    matches!(table, "settings" | "proxy_config")
}

#[derive(Default)]
struct Entries {
    generation: u64,
    proxy: HashMap<String, AppProxyConfig>,
    settings: HashMap<String, Option<String>>,
}

pub(crate) struct ConfigCache {
    generation: Arc<AtomicU64>,
    entries: Mutex<Entries>,
}

impl ConfigCache {
    pub(crate) fn new() -> Self {
        Self {
            generation: Arc::new(AtomicU64::new(0)),
            entries: Mutex::new(Entries::default()),
        }
    }

    /// 交给连接的 update hook：写入相关表时调用 `fetch_add` 即可让缓存失效
    pub(crate) fn generation_handle(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.generation)
    }

    pub(crate) fn invalidate(&self) {
        self.generation.fetch_add(1, Ordering::AcqRel);
    }

    fn lookup<T: Clone>(
        &self,
        select: fn(&mut Entries) -> &mut HashMap<String, T>,
        key: &str,
    ) -> (u64, Option<T>) {
        let generation = self.generation.load(Ordering::Acquire);
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.generation != generation {
            entries.proxy.clear();
            entries.settings.clear();
            entries.generation = generation;
            return (generation, None);
        }
        (generation, select(&mut entries).get(key).cloned())
    }

    fn store<T>(
        &self,
        select: fn(&mut Entries) -> &mut HashMap<String, T>,
        generation: u64,
        key: &str,
        value: T,
    ) {
        let mut entries = self.entries.lock().unwrap_or_else(PoisonError::into_inner);
        if entries.generation == generation && self.generation.load(Ordering::Acquire) == generation
        {
            select(&mut entries).insert(key.to_string(), value);
        }
    }

    /// 读 `settings` 表里 `key` 的值；未命中时用 `load` 查库并缓存
    pub(crate) fn setting<E>(
        &self,
        key: &str,
        load: impl FnOnce() -> Result<Option<String>, E>,
    ) -> Result<Option<String>, E> {
        let (generation, hit) = self.lookup(|e| &mut e.settings, key);
        if let Some(value) = hit {
            return Ok(value);
        }
        let value = load()?;
        self.store(|e| &mut e.settings, generation, key, value.clone());
        Ok(value)
    }

    /// 读应用级代理配置；未命中时用 `load` 查库。只缓存查到的行：行不存在
    /// （`None`）时调用方会补默认行，补写本身也会让缓存失效。查库出错不缓存。
    pub(crate) fn proxy_config<E>(
        &self,
        app_type: &str,
        load: impl FnOnce() -> Result<Option<AppProxyConfig>, E>,
    ) -> Result<Option<AppProxyConfig>, E> {
        let (generation, hit) = self.lookup(|e| &mut e.proxy, app_type);
        if hit.is_some() {
            return Ok(hit);
        }
        let config = load()?;
        if let Some(config) = &config {
            self.store(|e| &mut e.proxy, generation, app_type, config.clone());
        }
        Ok(config)
    }
}

impl Default for ConfigCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn read(cache: &ConfigCache, key: &str, value: &str, loads: &Cell<u32>) -> Option<String> {
        cache
            .setting::<()>(key, || {
                loads.set(loads.get() + 1);
                Ok(Some(value.to_string()))
            })
            .unwrap()
    }

    #[test]
    fn hits_until_invalidated() {
        let cache = ConfigCache::new();
        let loads = Cell::new(0);
        assert_eq!(read(&cache, "k", "a", &loads).as_deref(), Some("a"));
        assert_eq!(read(&cache, "k", "ignored", &loads).as_deref(), Some("a"));
        assert_eq!(loads.get(), 1);

        cache.invalidate();
        assert_eq!(read(&cache, "k", "b", &loads).as_deref(), Some("b"));
        assert_eq!(loads.get(), 2);
    }

    #[test]
    fn a_write_during_load_keeps_the_result_out_of_the_cache() {
        let cache = ConfigCache::new();
        let handle = cache.generation_handle();
        let value = cache
            .setting::<()>("k", || {
                // 模拟查库期间另一处写入了 settings 表
                handle.fetch_add(1, Ordering::AcqRel);
                Ok(Some("stale".to_string()))
            })
            .unwrap();
        assert_eq!(value.as_deref(), Some("stale"));

        let loads = Cell::new(0);
        assert_eq!(read(&cache, "k", "fresh", &loads).as_deref(), Some("fresh"));
        assert_eq!(loads.get(), 1, "the stale value must not have been cached");
    }

    #[test]
    fn errors_are_not_cached() {
        let cache = ConfigCache::new();
        assert!(cache.setting("k", || Err("db busy")).is_err());
        let loads = Cell::new(0);
        assert_eq!(read(&cache, "k", "ok", &loads).as_deref(), Some("ok"));
        assert_eq!(loads.get(), 1);
    }
}
