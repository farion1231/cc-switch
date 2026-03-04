//! 供应商路由器模块
//!
//! 负责选择和管理代理目标供应商，实现智能故障转移

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::{AllowResult, CircuitBreaker, CircuitBreakerConfig};
use chrono::Utc;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone)]
struct ProviderCooldown {
    until_unix: i64,
    reason: String,
}

/// 供应商路由器
pub struct ProviderRouter {
    /// 数据库连接
    db: Arc<Database>,
    /// 熔断器管理器 - key 格式: "app_type:provider_id"
    circuit_breakers: Arc<RwLock<HashMap<String, Arc<CircuitBreaker>>>>,
    /// Provider 冷却表 - key 格式: "app_type:provider_id"
    provider_cooldowns: Arc<RwLock<HashMap<String, ProviderCooldown>>>,
}

impl ProviderRouter {
    fn codex_quota_cooldown_remaining_secs(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Option<u64> {
        if app_type != "codex" {
            return None;
        }
        let account = self
            .db
            .get_codex_account_by_provider(provider_id)
            .ok()
            .flatten()?;
        let usage = self.db.get_codex_usage_state(&account.id).ok().flatten()?;
        if usage.allowed == Some(true) && usage.limit_reached == Some(false) {
            return None;
        }
        let from_secs = usage
            .primary_reset_after_seconds
            .unwrap_or(0)
            .max(usage.secondary_reset_after_seconds.unwrap_or(0));
        if from_secs > 0 {
            return Some(from_secs as u64);
        }
        let now = Utc::now().timestamp();
        let from_reset_at = (usage.primary_reset_at.unwrap_or(0) - now)
            .max(usage.secondary_reset_at.unwrap_or(0) - now);
        if from_reset_at > 0 {
            return Some(from_reset_at as u64);
        }
        // 有限额但缺少明确 reset 信息时给 60s 短冷却，防止请求风暴
        if usage.limit_reached == Some(true) {
            return Some(60);
        }
        None
    }

    fn gemini_quota_cooldown_remaining_secs(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Option<u64> {
        if app_type != "gemini" {
            return None;
        }
        let account = self
            .db
            .get_gemini_account_by_provider(provider_id)
            .ok()
            .flatten()?;
        let usage = self.db.get_gemini_usage_state(&account.id).ok().flatten()?;
        let cooldown_until = usage.cooldown_until?;
        let now = Utc::now().timestamp();
        (cooldown_until > now).then_some((cooldown_until - now) as u64)
    }

    /// 创建新的供应商路由器
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
            provider_cooldowns: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 选择可用的供应商（支持故障转移）
    ///
    /// 返回按优先级排序的可用供应商列表：
    /// - 故障转移关闭时：仅返回当前供应商
    /// - 故障转移开启时：优先使用故障转移队列；若队列供应商当前均不可用，自动回退到非队列供应商
    pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        let mut result = Vec::new();
        let mut total_providers = 0usize;
        let mut circuit_open_count = 0usize;
        let mut cooldown_count = 0usize;

        // 检查该应用的自动故障转移开关是否开启（从 proxy_config 表读取）
        let auto_failover_enabled = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(config) => config.auto_failover_enabled,
            Err(e) => {
                log::error!("[{app_type}] 读取 proxy_config 失败: {e}，默认禁用故障转移");
                false
            }
        };

        if auto_failover_enabled {
            // 故障转移开启：仅按队列顺序依次尝试（P1 → P2 → ...）
            let all_providers = self.db.get_all_providers(app_type)?;

            // 使用 DAO 返回的排序结果，确保和前端展示一致
            let ordered_ids: Vec<String> = self
                .db
                .get_failover_queue(app_type)?
                .into_iter()
                .map(|item| item.provider_id)
                .collect();

            let mut queued_provider_ids = HashSet::new();

            for provider_id in ordered_ids {
                queued_provider_ids.insert(provider_id.clone());
                let Some(provider) = all_providers.get(&provider_id).cloned() else {
                    continue;
                };
                total_providers += 1;
                let quota_cooldown = self
                    .codex_quota_cooldown_remaining_secs(&provider.id, app_type)
                    .or_else(|| self.gemini_quota_cooldown_remaining_secs(&provider.id, app_type));
                if let Some(remaining) = quota_cooldown
                {
                    cooldown_count += 1;
                    log::debug!(
                        "[{app_type}] Provider {} 额度窗口受限，剩余 {} 秒",
                        provider.name,
                        remaining
                    );
                    continue;
                }
                if let Some(remaining) = self
                    .provider_cooldown_remaining_secs(&provider.id, app_type)
                    .await
                {
                    cooldown_count += 1;
                    log::debug!(
                        "[{app_type}] Provider {} 处于冷却中，剩余 {} 秒",
                        provider.name,
                        remaining
                    );
                    continue;
                }

                let circuit_key = format!("{app_type}:{}", provider.id);
                let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

                if breaker.is_available().await {
                    result.push(provider);
                } else {
                    circuit_open_count += 1;
                }
            }

            // 队列中的 Provider 均不可用时，自动回退到非队列 Provider，避免误判“全熔断”
            if result.is_empty() {
                let mut fallback_providers: Vec<Provider> = all_providers
                    .iter()
                    .filter(|(id, _)| !queued_provider_ids.contains(*id))
                    .map(|(_, provider)| provider.clone())
                    .collect();

                fallback_providers.sort_by_key(|p| p.sort_index.unwrap_or(usize::MAX));

                if !fallback_providers.is_empty() {
                    log::warn!(
                        "[{app_type}] [FO-006] 故障转移队列当前不可用，回退到 {} 个非队列供应商",
                        fallback_providers.len()
                    );
                }

                for provider in fallback_providers {
                    total_providers += 1;
                    let quota_cooldown = self
                        .codex_quota_cooldown_remaining_secs(&provider.id, app_type)
                        .or_else(|| {
                            self.gemini_quota_cooldown_remaining_secs(&provider.id, app_type)
                        });
                    if let Some(remaining) = quota_cooldown
                    {
                        cooldown_count += 1;
                        log::debug!(
                            "[{app_type}] 回退 Provider {} 额度窗口受限，剩余 {} 秒",
                            provider.name,
                            remaining
                        );
                        continue;
                    }
                    if let Some(remaining) = self
                        .provider_cooldown_remaining_secs(&provider.id, app_type)
                        .await
                    {
                        cooldown_count += 1;
                        log::debug!(
                            "[{app_type}] 回退 Provider {} 处于冷却中，剩余 {} 秒",
                            provider.name,
                            remaining
                        );
                        continue;
                    }
                    let circuit_key = format!("{app_type}:{}", provider.id);
                    let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

                    if breaker.is_available().await {
                        result.push(provider);
                    } else {
                        circuit_open_count += 1;
                    }
                }
            }
        } else {
            // 故障转移关闭：仅使用当前供应商，跳过熔断器检查
            let current_id = AppType::from_str(app_type)
                .ok()
                .and_then(|app_enum| {
                    crate::settings::get_effective_current_provider(&self.db, &app_enum)
                        .ok()
                        .flatten()
                })
                .or_else(|| self.db.get_current_provider(app_type).ok().flatten());

            if let Some(current_id) = current_id {
                if let Some(current) = self.db.get_provider_by_id(&current_id, app_type)? {
                    total_providers = 1;
                    result.push(current);
                }
            }
        }

        if result.is_empty() {
            if total_providers > 0
                && (circuit_open_count + cooldown_count == total_providers)
                && (circuit_open_count > 0 || cooldown_count > 0)
            {
                log::warn!("[{app_type}] [FO-004] 所有供应商均已熔断");
                return Err(AppError::AllProvidersCircuitOpen);
            } else {
                log::warn!("[{app_type}] [FO-005] 未配置供应商");
                return Err(AppError::NoProvidersConfigured);
            }
        }

        Ok(result)
    }

    /// 请求执行前获取熔断器“放行许可”
    ///
    /// - Closed：直接放行
    /// - Open：超时到达后切到 HalfOpen 并放行一次探测
    /// - HalfOpen：按限流规则放行探测
    ///
    /// 注意：调用方必须在请求结束后通过 `record_result()` 释放 HalfOpen 名额，
    /// 否则会导致该 Provider 长时间无法进入探测状态。
    pub async fn allow_provider_request(&self, provider_id: &str, app_type: &str) -> AllowResult {
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
        breaker.allow_request().await
    }

    /// 记录供应商请求结果
    pub async fn record_result(
        &self,
        provider_id: &str,
        app_type: &str,
        used_half_open_permit: bool,
        success: bool,
        error_msg: Option<String>,
    ) -> Result<(), AppError> {
        // 1. 按应用独立获取熔断器配置
        let failure_threshold = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(app_config) => app_config.circuit_failure_threshold,
            Err(_) => 5, // 默认值
        };

        // 2. 更新熔断器状态
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

        if success {
            self.clear_provider_cooldown(provider_id, app_type).await;
            breaker.record_success(used_half_open_permit).await;
        } else {
            breaker.record_failure(used_half_open_permit).await;
        }

        // 3. 更新数据库健康状态（使用配置的阈值）
        self.db
            .update_provider_health_with_threshold(
                provider_id,
                app_type,
                success,
                error_msg.clone(),
                failure_threshold,
            )
            .await?;

        Ok(())
    }

    /// 重置熔断器（手动恢复）
    pub async fn reset_circuit_breaker(&self, circuit_key: &str) {
        let breakers = self.circuit_breakers.read().await;
        if let Some(breaker) = breakers.get(circuit_key) {
            breaker.reset().await;
        }
    }

    /// 重置指定供应商的熔断器
    pub async fn reset_provider_breaker(&self, provider_id: &str, app_type: &str) {
        let circuit_key = format!("{app_type}:{provider_id}");
        self.reset_circuit_breaker(&circuit_key).await;
    }

    /// 仅释放 HalfOpen permit，不影响健康统计（neutral 接口）
    ///
    /// 用于整流器等场景：请求结果不应计入 Provider 健康度，
    /// 但仍需释放占用的探测名额，避免 HalfOpen 状态卡死
    pub async fn release_permit_neutral(
        &self,
        provider_id: &str,
        app_type: &str,
        used_half_open_permit: bool,
    ) {
        if !used_half_open_permit {
            return;
        }
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
        breaker.release_half_open_permit();
    }

    /// 更新所有熔断器的配置（热更新）
    pub async fn update_all_configs(&self, config: CircuitBreakerConfig) {
        let breakers = self.circuit_breakers.read().await;
        for breaker in breakers.values() {
            breaker.update_config(config.clone()).await;
        }
    }

    /// 获取熔断器状态
    #[allow(dead_code)]
    pub async fn get_circuit_breaker_stats(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Option<crate::proxy::circuit_breaker::CircuitBreakerStats> {
        let circuit_key = format!("{app_type}:{provider_id}");
        let breakers = self.circuit_breakers.read().await;

        if let Some(breaker) = breakers.get(&circuit_key) {
            Some(breaker.get_stats().await)
        } else {
            None
        }
    }

    /// 获取或创建熔断器
    async fn get_or_create_circuit_breaker(&self, key: &str) -> Arc<CircuitBreaker> {
        // 先尝试读锁获取
        {
            let breakers = self.circuit_breakers.read().await;
            if let Some(breaker) = breakers.get(key) {
                return breaker.clone();
            }
        }

        // 如果不存在，获取写锁创建
        let mut breakers = self.circuit_breakers.write().await;

        // 双重检查，防止竞争条件
        if let Some(breaker) = breakers.get(key) {
            return breaker.clone();
        }

        // 从 key 中提取 app_type (格式: "app_type:provider_id")
        let app_type = key.split(':').next().unwrap_or("claude");

        // 按应用独立读取熔断器配置
        let config = match self.db.get_proxy_config_for_app(app_type).await {
            Ok(app_config) => crate::proxy::circuit_breaker::CircuitBreakerConfig {
                failure_threshold: app_config.circuit_failure_threshold,
                success_threshold: app_config.circuit_success_threshold,
                timeout_seconds: app_config.circuit_timeout_seconds as u64,
                error_rate_threshold: app_config.circuit_error_rate_threshold,
                min_requests: app_config.circuit_min_requests,
            },
            Err(_) => crate::proxy::circuit_breaker::CircuitBreakerConfig::default(),
        };

        let breaker = Arc::new(CircuitBreaker::new(config));
        breakers.insert(key.to_string(), breaker.clone());

        breaker
    }

    /// 设置 Provider 冷却到指定 Unix 时间（秒）
    pub async fn set_provider_cooldown_until(
        &self,
        provider_id: &str,
        app_type: &str,
        until_unix: i64,
        reason: impl Into<String>,
    ) {
        let now = Utc::now().timestamp();
        if until_unix <= now {
            self.clear_provider_cooldown(provider_id, app_type).await;
            return;
        }

        let key = format!("{app_type}:{provider_id}");
        let mut map = self.provider_cooldowns.write().await;
        map.insert(
            key,
            ProviderCooldown {
                until_unix,
                reason: reason.into(),
            },
        );
    }

    /// 设置 Provider 冷却一段秒数
    pub async fn set_provider_cooldown_for_secs(
        &self,
        provider_id: &str,
        app_type: &str,
        cooldown_secs: u64,
        reason: impl Into<String>,
    ) {
        let until_unix = Utc::now().timestamp() + cooldown_secs as i64;
        self.set_provider_cooldown_until(provider_id, app_type, until_unix, reason)
            .await;
    }

    /// 清理 Provider 冷却状态
    pub async fn clear_provider_cooldown(&self, provider_id: &str, app_type: &str) {
        let key = format!("{app_type}:{provider_id}");
        let mut map = self.provider_cooldowns.write().await;
        map.remove(&key);
    }

    /// 获取剩余冷却秒数（若已过期会自动清理并返回 None）
    async fn provider_cooldown_remaining_secs(
        &self,
        provider_id: &str,
        app_type: &str,
    ) -> Option<u64> {
        let key = format!("{app_type}:{provider_id}");
        let now = Utc::now().timestamp();

        let mut map = self.provider_cooldowns.write().await;
        if let Some(cooldown) = map.get(&key) {
            if cooldown.until_unix > now {
                let remaining = (cooldown.until_unix - now) as u64;
                let _reason = &cooldown.reason;
                return Some(remaining);
            }
            map.remove(&key);
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
            }
        }
    }

    impl Drop for TempHome {
        fn drop(&mut self) {
            match &self.original_home {
                Some(value) => env::set_var("HOME", value),
                None => env::remove_var("HOME"),
            }

            match &self.original_userprofile {
                Some(value) => env::set_var("USERPROFILE", value),
                None => env::remove_var("USERPROFILE"),
            }
        }
    }

    #[tokio::test]
    #[serial]
    async fn test_provider_router_creation() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let router = ProviderRouter::new(db);

        let breaker = router.get_or_create_circuit_breaker("claude:test").await;
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_disabled_uses_current_provider() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "b").unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_enabled_uses_queue_order_ignoring_current() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 设置 sort_index 来控制顺序：b=1, a=2
        let mut provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        provider_a.sort_index = Some(2);
        let mut provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        provider_b.sort_index = Some(1);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        db.add_to_failover_queue("claude", "b").unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();

        // 启用自动故障转移（使用新的 proxy_config API）
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 2);
        // 故障转移开启时：仅按队列顺序选择（忽略当前供应商）
        assert_eq!(providers[0].id, "b");
        assert_eq!(providers[1].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_enabled_uses_queue_only_even_if_current_not_in_queue() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let mut provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        provider_b.sort_index = Some(1);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        // 只把 b 加入故障转移队列（模拟“当前供应商不在队列里”的常见配置）
        db.add_to_failover_queue("claude", "b").unwrap();

        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "b");
    }

    #[tokio::test]
    #[serial]
    async fn test_failover_enabled_fallbacks_to_non_queue_provider_when_queue_unavailable() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        // 仅将 b 放入故障转移队列
        db.add_to_failover_queue("claude", "b").unwrap();

        // 打开自动故障转移并将失败阈值设置为 1，方便快速触发熔断
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        config.circuit_failure_threshold = 1;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 触发 b 熔断
        router
            .record_result("b", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        // 队列里的 b 不可用时，应自动回退到非队列的 a
        let providers = router.select_providers("claude").await.unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_select_providers_does_not_consume_half_open_permit() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);

        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();

        db.add_to_failover_queue("claude", "a").unwrap();
        db.add_to_failover_queue("claude", "b").unwrap();

        // 启用自动故障转移（使用新的 proxy_config API）
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        router
            .record_result("b", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        let providers = router.select_providers("claude").await.unwrap();
        assert_eq!(providers.len(), 2);

        assert!(router.allow_provider_request("b", "claude").await.allowed);
    }

    #[tokio::test]
    #[serial]
    async fn test_release_permit_neutral_frees_half_open_slot() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 配置熔断器：1 次失败即熔断，0 秒超时立即进入 HalfOpen
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 0,
            ..Default::default()
        })
        .await
        .unwrap();

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.add_to_failover_queue("claude", "a").unwrap();

        // 启用自动故障转移
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.auto_failover_enabled = true;
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 触发熔断：1 次失败
        router
            .record_result("a", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        // 第一次请求：获取 HalfOpen 探测名额
        let first = router.allow_provider_request("a", "claude").await;
        assert!(first.allowed);
        assert!(first.used_half_open_permit);

        // 第二次请求应被拒绝（名额已被占用）
        let second = router.allow_provider_request("a", "claude").await;
        assert!(!second.allowed);

        // 使用 release_permit_neutral 释放名额（不影响健康统计）
        router
            .release_permit_neutral("a", "claude", first.used_half_open_permit)
            .await;

        // 第三次请求应被允许（名额已释放）
        let third = router.allow_provider_request("a", "claude").await;
        assert!(third.allowed);
        assert!(third.used_half_open_permit);
    }
}
