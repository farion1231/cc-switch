//! 托盘展示用的用量缓存（进程内、写穿式）。
//!
//! 各 usage 查询命令成功时写入；系统托盘构建菜单时读取。不持久化，
//! 进程重启即空，由下一次自动查询或「刷新所有用量」动作重新填充。

use std::collections::HashMap;
use std::sync::RwLock;
use std::time::Instant;

use crate::app_config::AppType;
use crate::provider::UsageResult;
use crate::services::subscription::SubscriptionQuota;

#[derive(Debug, Clone)]
pub struct CachedSubscription {
    pub quota: SubscriptionQuota,
    pub cached_at: Instant,
}

#[derive(Debug, Clone)]
pub struct CachedScript {
    pub result: UsageResult,
    pub cached_at: Instant,
}

#[derive(Default)]
pub struct UsageCache {
    subscription: RwLock<HashMap<AppType, CachedSubscription>>,
    script: RwLock<HashMap<(AppType, String), CachedScript>>,
}

impl UsageCache {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn put_subscription(&self, app_type: AppType, quota: SubscriptionQuota) {
        if let Ok(mut w) = self.subscription.write() {
            w.insert(
                app_type,
                CachedSubscription {
                    quota,
                    cached_at: Instant::now(),
                },
            );
        }
    }

    pub fn get_subscription(&self, app_type: &AppType) -> Option<CachedSubscription> {
        self.subscription
            .read()
            .ok()
            .and_then(|r| r.get(app_type).cloned())
    }

    pub fn put_script(&self, app_type: AppType, provider_id: String, result: UsageResult) {
        if let Ok(mut w) = self.script.write() {
            w.insert(
                (app_type, provider_id),
                CachedScript {
                    result,
                    cached_at: Instant::now(),
                },
            );
        }
    }

    pub fn get_script(&self, app_type: &AppType, provider_id: &str) -> Option<CachedScript> {
        self.script
            .read()
            .ok()
            .and_then(|r| r.get(&(app_type.clone(), provider_id.to_string())).cloned())
    }

    pub fn invalidate_script(&self, app_type: &AppType, provider_id: &str) {
        if let Ok(mut w) = self.script.write() {
            w.remove(&(app_type.clone(), provider_id.to_string()));
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
        assert!(cache.get_subscription(&AppType::Claude).is_none());
        cache.put_subscription(AppType::Claude, fake_quota());
        let got = cache.get_subscription(&AppType::Claude).unwrap();
        assert!(got.quota.success);
        assert!(cache.get_subscription(&AppType::Codex).is_none());
    }

    #[test]
    fn script_round_trip_and_invalidate() {
        let cache = UsageCache::new();
        assert!(cache.get_script(&AppType::Codex, "pid").is_none());
        cache.put_script(AppType::Codex, "pid".to_string(), fake_result());
        assert!(cache.get_script(&AppType::Codex, "pid").is_some());
        cache.invalidate_script(&AppType::Codex, "pid");
        assert!(cache.get_script(&AppType::Codex, "pid").is_none());
    }

    #[test]
    fn script_keys_isolated_by_app_type() {
        let cache = UsageCache::new();
        cache.put_script(AppType::Claude, "same".to_string(), fake_result());
        assert!(cache.get_script(&AppType::Claude, "same").is_some());
        assert!(cache.get_script(&AppType::Codex, "same").is_none());
    }
}
