//! 托盘展示用的用量缓存（进程内、写穿式）。
//!
//! 各 usage 查询命令成功时写入；系统托盘构建菜单时读取。不持久化，
//! 进程重启即空，由下一次自动查询或托盘悬停触发的刷新重新填充。

use std::collections::HashMap;
use std::sync::RwLock;

use crate::app_config::AppType;
use crate::provider::UsageResult;
use crate::services::subscription::SubscriptionQuota;

#[derive(Default)]
pub struct UsageCache {
    subscription: RwLock<HashMap<AppType, SubscriptionQuota>>,
    codex_oauth: RwLock<HashMap<String, CodexOauthCacheEntry>>,
    script: RwLock<HashMap<(AppType, String), UsageResult>>,
}

#[derive(Default)]
struct CodexOauthCacheEntry {
    generation: u64,
    quota: Option<SubscriptionQuota>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum CodexOauthCachePublication {
    Published,
    Superseded,
    Stale,
    Unavailable,
}

impl UsageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_subscription(&self, app_type: AppType, quota: SubscriptionQuota) {
        if let Ok(mut w) = self.subscription.write() {
            w.insert(app_type, quota);
        }
    }

    pub fn put_codex_oauth(&self, account_id: String, quota: SubscriptionQuota) -> bool {
        let generation = self.codex_oauth_generation(&account_id);
        matches!(
            self.put_codex_oauth_if_generation_result(account_id, generation, quota),
            CodexOauthCachePublication::Published
        )
    }

    pub fn codex_oauth_generation(&self, account_id: &str) -> u64 {
        self.codex_oauth
            .write()
            .ok()
            .map(|mut accounts| {
                accounts
                    .entry(account_id.to_string())
                    .or_default()
                    .generation
            })
            .unwrap_or(0)
    }

    /// Publish a quota only if no account lifecycle operation invalidated the
    /// generation captured before the request started.
    pub fn put_codex_oauth_if_generation(
        &self,
        account_id: String,
        expected_generation: u64,
        quota: SubscriptionQuota,
    ) -> bool {
        matches!(
            self.put_codex_oauth_if_generation_result(account_id, expected_generation, quota),
            CodexOauthCachePublication::Published
        )
    }

    pub(crate) fn put_codex_oauth_if_generation_result(
        &self,
        account_id: String,
        expected_generation: u64,
        quota: SubscriptionQuota,
    ) -> CodexOauthCachePublication {
        if let Ok(mut accounts) = self.codex_oauth.write() {
            let entry = accounts.entry(account_id).or_default();
            if entry.generation != expected_generation {
                return CodexOauthCachePublication::Superseded;
            }
            if let Some(previous) = entry.quota.as_ref() {
                let is_older = match (previous.queried_at, quota.queried_at) {
                    (Some(previous), Some(incoming)) => incoming < previous,
                    (Some(_), None) => true,
                    _ => false,
                };
                if is_older {
                    return CodexOauthCachePublication::Stale;
                }
            }
            entry.quota = Some(quota);
            return CodexOauthCachePublication::Published;
        }
        CodexOauthCachePublication::Unavailable
    }

    /// Invalidate a single managed account's in-flight quota publication.
    pub fn invalidate_codex_oauth(&self, account_id: &str) {
        if let Ok(mut accounts) = self.codex_oauth.write() {
            let entry = accounts.entry(account_id.to_string()).or_default();
            entry.quota = None;
            entry.generation = entry.generation.wrapping_add(1);
        }
    }

    /// Managed accounts must never share the CLI's app-wide subscription snapshot.
    pub fn with_codex_oauth<R>(
        &self,
        account_id: &str,
        f: impl FnOnce(&SubscriptionQuota) -> R,
    ) -> Option<R> {
        self.codex_oauth.read().ok().and_then(|r| {
            r.get(account_id)
                .and_then(|entry| entry.quota.as_ref().map(f))
        })
    }

    pub fn put_script(&self, app_type: AppType, provider_id: String, result: UsageResult) {
        if let Ok(mut w) = self.script.write() {
            w.insert((app_type, provider_id), result);
        }
    }

    /// 以借用形式暴露订阅快照，避免托盘每次重建时深拷贝整个 `SubscriptionQuota`。
    pub fn with_subscription<R>(
        &self,
        app_type: &AppType,
        f: impl FnOnce(&SubscriptionQuota) -> R,
    ) -> Option<R> {
        self.subscription
            .read()
            .ok()
            .and_then(|r| r.get(app_type).map(f))
    }

    /// 以借用形式暴露脚本型用量结果，同上。
    pub fn with_script<R>(
        &self,
        app_type: &AppType,
        provider_id: &str,
        f: impl FnOnce(&UsageResult) -> R,
    ) -> Option<R> {
        self.script
            .read()
            .ok()
            .and_then(|r| r.get(&(app_type.clone(), provider_id.to_string())).map(f))
    }

    pub fn invalidate_script(&self, app_type: &AppType, provider_id: &str) {
        // 热路径会对每个禁用脚本的 provider 在托盘重建时调用一次：先走读锁
        // `contains_key` 快速放行"本来就不在缓存里"的常见情况，避免无谓的写锁升级。
        let key = (app_type.clone(), provider_id.to_string());
        if !self.script.read().is_ok_and(|r| r.contains_key(&key)) {
            return;
        }
        if let Ok(mut w) = self.script.write() {
            w.remove(&key);
        }
    }

    pub fn invalidate_subscription(&self, app_type: &AppType) {
        if !self
            .subscription
            .read()
            .is_ok_and(|r| r.contains_key(app_type))
        {
            return;
        }
        if let Ok(mut w) = self.subscription.write() {
            w.remove(app_type);
        }
    }

    /// Drop all process-local usage snapshots after a database/provider restore.
    pub fn invalidate_all(&self) {
        if let Ok(mut subscriptions) = self.subscription.write() {
            subscriptions.clear();
        }
        if let Ok(mut scripts) = self.script.write() {
            scripts.clear();
        }
        if let Ok(mut accounts) = self.codex_oauth.write() {
            for entry in accounts.values_mut() {
                entry.quota = None;
                entry.generation = entry.generation.wrapping_add(1);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::subscription::CredentialStatus;

    fn fake_quota() -> SubscriptionQuota {
        SubscriptionQuota {
            tool: "claude".to_string(),
            credential_status: CredentialStatus::Valid,
            credential_message: None,
            success: true,
            tiers: vec![],
            extra_usage: None,
            reset_credits: None,
            credits_balance: None,
            error: None,
            queried_at: Some(0),
        }
    }

    fn fake_result() -> UsageResult {
        UsageResult {
            success: true,
            data: None,
            error: None,
        }
    }

    #[test]
    fn subscription_round_trip() {
        let cache = UsageCache::new();
        assert!(cache
            .with_subscription(&AppType::Claude, |q| q.success)
            .is_none());
        cache.put_subscription(AppType::Claude, fake_quota());
        let got = cache
            .with_subscription(&AppType::Claude, |q| q.success)
            .unwrap();
        assert!(got);
        assert!(cache
            .with_subscription(&AppType::Codex, |q| q.success)
            .is_none());
    }

    #[test]
    fn script_round_trip_and_invalidate() {
        let cache = UsageCache::new();
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_none());
        cache.put_script(AppType::Codex, "pid".to_string(), fake_result());
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_some());
        cache.invalidate_script(&AppType::Codex, "pid");
        assert!(cache
            .with_script(&AppType::Codex, "pid", |r| r.success)
            .is_none());
    }

    #[test]
    fn script_keys_isolated_by_app_type() {
        let cache = UsageCache::new();
        cache.put_script(AppType::Claude, "same".to_string(), fake_result());
        assert!(cache
            .with_script(&AppType::Claude, "same", |r| r.success)
            .is_some());
        assert!(cache
            .with_script(&AppType::Codex, "same", |r| r.success)
            .is_none());
    }

    #[test]
    fn invalidate_all_clears_all_usage_snapshots() {
        let cache = UsageCache::new();
        cache.put_subscription(AppType::Claude, fake_quota());
        cache.put_codex_oauth("account-1".to_string(), fake_quota());
        cache.put_script(AppType::Codex, "provider".to_string(), fake_result());

        cache.invalidate_all();

        assert!(cache
            .with_subscription(&AppType::Claude, |quota| quota.success)
            .is_none());
        assert!(cache
            .with_script(&AppType::Codex, "provider", |usage| usage.success)
            .is_none());
        assert!(cache
            .with_codex_oauth("account-1", |quota| quota.success)
            .is_none());
    }

    #[test]
    fn codex_oauth_cache_rejects_late_older_observation() {
        let cache = UsageCache::new();
        let mut newer = fake_quota();
        newer.queried_at = Some(20);
        let mut older = fake_quota();
        older.queried_at = Some(0);
        assert!(cache.put_codex_oauth("account".to_string(), newer.clone()));
        assert!(!cache.put_codex_oauth("account".to_string(), older));
        assert_eq!(
            cache.with_codex_oauth("account", |quota| quota.queried_at),
            Some(Some(20))
        );
    }

    #[test]
    fn codex_oauth_generation_rejects_results_after_account_invalidation() {
        let cache = UsageCache::new();
        let generation = cache.codex_oauth_generation("account");
        cache.invalidate_codex_oauth("account");
        assert!(!cache.put_codex_oauth_if_generation(
            "account".to_string(),
            generation,
            fake_quota(),
        ));
        assert!(cache.with_codex_oauth("account", |_| true).is_none());
    }

    #[test]
    fn invalidate_all_rejects_an_inflight_account_seen_only_by_generation() {
        let cache = UsageCache::new();
        let generation = cache.codex_oauth_generation("inflight-only");
        cache.invalidate_all();
        assert!(!cache.put_codex_oauth_if_generation(
            "inflight-only".to_string(),
            generation,
            fake_quota(),
        ));
    }

    #[test]
    fn invalidation_and_old_publisher_are_serialized_by_one_entry_lock() {
        use std::sync::{Arc, Barrier};

        let cache = Arc::new(UsageCache::new());
        let generation = cache.codex_oauth_generation("account");
        let barrier = Arc::new(Barrier::new(2));
        let publisher_cache = cache.clone();
        let publisher_barrier = barrier.clone();
        let publisher = std::thread::spawn(move || {
            publisher_barrier.wait();
            publisher_cache.put_codex_oauth_if_generation(
                "account".to_string(),
                generation,
                fake_quota(),
            )
        });
        barrier.wait();
        cache.invalidate_codex_oauth("account");
        let _ = publisher.join().expect("publisher thread should finish");
        assert!(cache.with_codex_oauth("account", |_| true).is_none());
        let next_generation = cache.codex_oauth_generation("account");
        assert!(cache.put_codex_oauth_if_generation(
            "account".to_string(),
            next_generation,
            fake_quota(),
        ));
    }
}
