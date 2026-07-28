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
    /// Codex 多账号用量缓存：account_key -> SubscriptionQuota
    codex_accounts: RwLock<HashMap<String, SubscriptionQuota>>,
    script: RwLock<HashMap<(AppType, String), UsageResult>>,
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

    pub fn put_script(&self, app_type: AppType, provider_id: String, result: UsageResult) {
        if let Ok(mut w) = self.script.write() {
            w.insert((app_type, provider_id), result);
        }
    }

    // ── Codex 多账号缓存 ────────────────────────────────────

    pub fn put_codex_account(&self, account_key: String, quota: SubscriptionQuota) {
        if let Ok(mut w) = self.codex_accounts.write() {
            w.insert(account_key, quota);
        }
    }

    pub fn get_codex_account(&self, account_key: &str) -> Option<SubscriptionQuota> {
        self.codex_accounts
            .read()
            .ok()
            .and_then(|r| r.get(account_key).cloned())
    }

    pub fn with_codex_account<R>(
        &self,
        account_key: &str,
        f: impl FnOnce(&SubscriptionQuota) -> R,
    ) -> Option<R> {
        self.codex_accounts
            .read()
            .ok()
            .and_then(|r| r.get(account_key).map(f))
    }

    pub fn get_all_codex_accounts(&self) -> HashMap<String, SubscriptionQuota> {
        self.codex_accounts
            .read()
            .ok()
            .map(|r| r.clone())
            .unwrap_or_default()
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
}
