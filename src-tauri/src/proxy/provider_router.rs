//! 供应商路由器模块
//!
//! 负责选择和管理代理目标供应商，实现智能故障转移

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::{AllowResult, CircuitBreaker, CircuitBreakerConfig};
use crate::proxy::model_mapper::ClaudeTier;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use tokio::sync::RwLock;

/// 供应商路由器
pub struct ProviderRouter {
    /// 数据库连接
    db: Arc<Database>,
    /// 熔断器管理器 - key 格式: "app_type:provider_id"
    circuit_breakers: Arc<RwLock<HashMap<String, Arc<CircuitBreaker>>>>,
}

/// 聚合供应商路由展开结果
pub struct AggregateExpansion {
    /// 展开后的 provider 链（聚合供应商按档位替换为目标 provider）
    pub providers: Vec<Provider>,
    /// 由聚合路由合成的 provider id → 来源聚合供应商 (id, name)。
    /// 供 forwarder 决定“当前供应商”切换目标：同一聚合的各档目标之间不切换，
    /// 但故障转移首次命中聚合时，把当前供应商同步到该聚合供应商。
    pub routed_provider_sources: HashMap<String, (String, String)>,
}

impl ProviderRouter {
    /// 创建新的供应商路由器
    pub fn new(db: Arc<Database>) -> Self {
        Self {
            db,
            circuit_breakers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 选择可用的供应商（支持故障转移）
    ///
    /// 返回按优先级排序的可用供应商列表：
    /// - 故障转移关闭时：仅返回当前供应商
    /// - 故障转移开启时：仅使用故障转移队列，按队列顺序依次尝试（P1 → P2 → ...）
    pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        let mut result = Vec::new();
        let mut total_providers = 0usize;
        let mut circuit_open_count = 0usize;

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

            total_providers = ordered_ids.len();

            for provider_id in ordered_ids {
                let Some(provider) = all_providers.get(&provider_id).cloned() else {
                    continue;
                };

                let circuit_key = format!("{app_type}:{}", provider.id);
                let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

                if breaker.is_available().await {
                    result.push(provider);
                } else {
                    circuit_open_count += 1;
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
            if total_providers > 0 && circuit_open_count == total_providers {
                log::warn!("[{app_type}] [FO-004] 所有供应商均已熔断");
                return Err(AppError::AllProvidersCircuitOpen);
            } else {
                log::warn!("[{app_type}] [FO-005] 未配置供应商");
                return Err(AppError::NoProvidersConfigured);
            }
        }

        Ok(result)
    }

    /// 展开链上的聚合供应商：按请求模型的档位（tier）将其替换为目标 provider。
    ///
    /// - 非聚合 provider 原样保留；
    /// - 聚合 provider 在以下情况从链上丢弃（并 log）：模型无法分类 / 该档未配置路由、
    ///   目标 provider 已删除、目标也是聚合供应商（禁止嵌套）；
    /// - `check_breaker` 为 true（故障转移开启）时，目标熔断中同样丢弃，
    ///   由链上后续 provider 自动回退。
    ///
    /// 命中路由时克隆目标 provider 并改写其模型 env（见
    /// [`Self::synthesize_routed_provider`]），目标 id → 来源聚合供应商记入返回的
    /// `routed_provider_sources`，供 forwarder 决定"当前供应商"切换目标。
    /// 同一 id 只保留首次出现（去重）。
    pub async fn expand_aggregate_routes(
        &self,
        providers: Vec<Provider>,
        tier: Option<ClaudeTier>,
        app_type: &str,
        check_breaker: bool,
    ) -> AggregateExpansion {
        let mut result: Vec<Provider> = Vec::with_capacity(providers.len());
        let mut seen_ids: HashSet<String> = HashSet::new();
        let mut routed_sources: HashMap<String, (String, String)> = HashMap::new();

        for provider in providers {
            let Some(routes) = provider.aggregate_routes() else {
                if seen_ids.insert(provider.id.clone()) {
                    result.push(provider);
                }
                continue;
            };

            let route = tier.and_then(|t| match t {
                ClaudeTier::Haiku => routes.haiku.as_ref(),
                ClaudeTier::Sonnet => routes.sonnet.as_ref(),
                ClaudeTier::Opus => routes.opus.as_ref(),
                ClaudeTier::Fable => routes.fable.as_ref(),
            });

            let Some(route) = route else {
                log::warn!(
                    "[{app_type}] 聚合供应商 {} 未配置 {:?} 档路由，跳过",
                    provider.name,
                    tier
                );
                continue;
            };

            let target_id = route.provider_id.trim();
            let target_model = route.model.trim();
            if target_id.is_empty() || target_model.is_empty() {
                log::warn!(
                    "[{app_type}] 聚合供应商 {} 的 {:?} 档路由不完整，跳过",
                    provider.name,
                    tier
                );
                continue;
            }

            let target = match self.db.get_provider_by_id(target_id, app_type) {
                Ok(Some(p)) => p,
                Ok(None) => {
                    log::warn!(
                        "[{app_type}] 聚合供应商 {} 的 {:?} 档目标 provider {} 已删除，跳过",
                        provider.name,
                        tier,
                        target_id
                    );
                    continue;
                }
                Err(e) => {
                    log::error!(
                        "[{app_type}] 读取聚合路由目标 provider {} 失败: {e}，跳过",
                        target_id
                    );
                    continue;
                }
            };

            if target.is_aggregate() {
                log::warn!(
                    "[{app_type}] 聚合供应商 {} 的 {:?} 档目标 {} 也是聚合供应商，跳过（禁止嵌套）",
                    provider.name,
                    tier,
                    target.name
                );
                continue;
            }

            if check_breaker {
                let circuit_key = format!("{app_type}:{}", target.id);
                let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
                if !breaker.is_available().await {
                    log::warn!(
                        "[{app_type}] 聚合路由目标 {} 熔断中，跳过（自动回退到链上后续 provider）",
                        target.name
                    );
                    continue;
                }
            }

            log::debug!(
                "[{app_type}] 聚合路由: {} {:?} 档 → {} (model={})",
                provider.name,
                tier,
                target.name,
                target_model
            );
            let routed = Self::synthesize_routed_provider(&target, target_model);
            if seen_ids.insert(routed.id.clone()) {
                routed_sources.insert(
                    routed.id.clone(),
                    (provider.id.clone(), provider.name.clone()),
                );
                result.push(routed);
            }
        }

        AggregateExpansion {
            providers: result,
            routed_provider_sources: routed_sources,
        }
    }

    /// 克隆目标 provider 并覆写模型 env：清除分层/子代理模型键，将
    /// `ANTHROPIC_MODEL` 设为路由模型名。下游 model_mapper 因此会把任意档别名
    /// 统一映射到该模型，目标 provider 的端点/认证/归一化逻辑全部复用。
    fn synthesize_routed_provider(target: &Provider, model: &str) -> Provider {
        const TIER_MODEL_KEYS: [&str; 6] = [
            "ANTHROPIC_DEFAULT_HAIKU_MODEL",
            "ANTHROPIC_DEFAULT_SONNET_MODEL",
            "ANTHROPIC_DEFAULT_OPUS_MODEL",
            "ANTHROPIC_DEFAULT_FABLE_MODEL",
            "ANTHROPIC_SMALL_FAST_MODEL",
            "CLAUDE_CODE_SUBAGENT_MODEL",
        ];

        let mut routed = target.clone();
        if !routed.settings_config.is_object() {
            routed.settings_config = serde_json::json!({});
        }
        let root = routed
            .settings_config
            .as_object_mut()
            .expect("settings_config normalized to object");
        let env_value = root
            .entry("env".to_string())
            .or_insert_with(|| serde_json::json!({}));
        if !env_value.is_object() {
            *env_value = serde_json::json!({});
        }
        let env = env_value
            .as_object_mut()
            .expect("settings_config.env normalized to object");

        for key in TIER_MODEL_KEYS {
            env.remove(key);
        }
        env.insert(
            "ANTHROPIC_MODEL".to_string(),
            serde_json::Value::String(model.to_string()),
        );
        routed
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

    /// 更新指定应用已创建熔断器的配置（热更新）
    pub async fn update_app_configs(&self, app_type: &str, config: CircuitBreakerConfig) {
        let prefix = format!("{app_type}:");
        let breakers = self.circuit_breakers.read().await;
        for (key, breaker) in breakers.iter() {
            if key.starts_with(&prefix) {
                breaker.update_config(config.clone()).await;
            }
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::provider::{AggregateRoute, AggregateRoutes, ProviderMeta};
    use serde_json::json;
    use serial_test::serial;
    use std::env;
    use tempfile::TempDir;

    struct TempHome {
        #[allow(dead_code)]
        dir: TempDir,
        original_home: Option<String>,
        original_userprofile: Option<String>,
        original_test_home: Option<String>,
    }

    impl TempHome {
        fn new() -> Self {
            let dir = TempDir::new().expect("failed to create temp home");
            let original_home = env::var("HOME").ok();
            let original_userprofile = env::var("USERPROFILE").ok();
            let original_test_home = env::var("CC_SWITCH_TEST_HOME").ok();

            env::set_var("HOME", dir.path());
            env::set_var("USERPROFILE", dir.path());
            env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            crate::settings::reload_settings().expect("reload settings");

            Self {
                dir,
                original_home,
                original_userprofile,
                original_test_home,
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

            match &self.original_test_home {
                Some(value) => env::set_var("CC_SWITCH_TEST_HOME", value),
                None => env::remove_var("CC_SWITCH_TEST_HOME"),
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

    // ==================== 聚合供应商路由展开 ====================

    fn make_aggregate_provider(id: &str, routes: AggregateRoutes) -> Provider {
        let mut provider =
            Provider::with_id(id.to_string(), format!("Aggregate {id}"), json!({}), None);
        let mut meta = ProviderMeta::default();
        meta.aggregate_routes = Some(routes);
        provider.meta = Some(meta);
        provider
    }

    fn single_fable_route(target_id: &str, model: &str) -> AggregateRoutes {
        let mut routes = AggregateRoutes::default();
        routes.fable = Some(AggregateRoute {
            provider_id: target_id.to_string(),
            model: model.to_string(),
        });
        routes
    }

    #[tokio::test]
    #[serial]
    async fn test_expand_aggregate_routes_hits_target_and_overrides_model_env() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let kimi = Provider::with_id(
            "kimi".to_string(),
            "Kimi".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "sk-kimi",
                    "ANTHROPIC_BASE_URL": "https://api.kimi.com/coding",
                    "ANTHROPIC_MODEL": "k2",
                    "ANTHROPIC_DEFAULT_FABLE_MODEL": "k3-fable",
                    "ANTHROPIC_DEFAULT_HAIKU_MODEL": "k3-haiku",
                    "CLAUDE_CODE_SUBAGENT_MODEL": "k3-sub"
                }
            }),
            None,
        );
        db.save_provider("claude", &kimi).unwrap();

        let agg = make_aggregate_provider("agg", single_fable_route("kimi", "k3"));

        let router = ProviderRouter::new(db.clone());
        let expansion = router
            .expand_aggregate_routes(vec![agg], Some(ClaudeTier::Fable), "claude", false)
            .await;

        assert_eq!(expansion.providers.len(), 1);
        let routed = &expansion.providers[0];
        assert_eq!(routed.id, "kimi");
        assert_eq!(
            expansion.routed_provider_sources.get("kimi"),
            Some(&("agg".to_string(), "Aggregate agg".to_string())),
            "routed target should record its source aggregate provider"
        );

        let env = routed.settings_config.get("env").unwrap();
        assert_eq!(env.get("ANTHROPIC_MODEL").unwrap(), "k3");
        // 分层/子代理键被清除，避免目标 provider 自身的档配置覆盖路由模型
        assert!(env.get("ANTHROPIC_DEFAULT_FABLE_MODEL").is_none());
        assert!(env.get("ANTHROPIC_DEFAULT_HAIKU_MODEL").is_none());
        assert!(env.get("CLAUDE_CODE_SUBAGENT_MODEL").is_none());
        assert!(env.get("ANTHROPIC_SMALL_FAST_MODEL").is_none());
        // 端点与密钥原样保留
        assert_eq!(env.get("ANTHROPIC_AUTH_TOKEN").unwrap(), "sk-kimi");
        assert_eq!(
            env.get("ANTHROPIC_BASE_URL").unwrap(),
            "https://api.kimi.com/coding"
        );

        // 合成后的 provider 经 model_mapper 会把任意档别名映射到路由模型
        let (mapped, _, _) = crate::proxy::model_mapper::apply_model_mapping(
            json!({"model": "claude-fable-5"}),
            routed,
        );
        assert_eq!(mapped["model"], "k3");
    }

    #[tokio::test]
    #[serial]
    async fn test_expand_aggregate_routes_drops_unconfigured_tier() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let kimi = Provider::with_id("kimi".to_string(), "Kimi".to_string(), json!({}), None);
        db.save_provider("claude", &kimi).unwrap();

        let agg = make_aggregate_provider("agg", single_fable_route("kimi", "k3"));

        let router = ProviderRouter::new(db.clone());
        // 只配置了 Fable 档，Haiku 档请求 → 聚合供应商被丢弃
        let expansion = router
            .expand_aggregate_routes(vec![agg], Some(ClaudeTier::Haiku), "claude", false)
            .await;

        assert!(expansion.providers.is_empty());
        assert!(expansion.routed_provider_sources.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_expand_aggregate_routes_drops_unclassifiable_model() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let kimi = Provider::with_id("kimi".to_string(), "Kimi".to_string(), json!({}), None);
        db.save_provider("claude", &kimi).unwrap();

        let agg = make_aggregate_provider("agg", single_fable_route("kimi", "k3"));

        let router = ProviderRouter::new(db.clone());
        let expansion = router
            .expand_aggregate_routes(vec![agg], None, "claude", false)
            .await;

        assert!(expansion.providers.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_expand_aggregate_routes_drops_missing_target() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let agg = make_aggregate_provider("agg", single_fable_route("ghost", "k3"));

        let router = ProviderRouter::new(db.clone());
        let expansion = router
            .expand_aggregate_routes(vec![agg], Some(ClaudeTier::Fable), "claude", false)
            .await;

        assert!(expansion.providers.is_empty());
        assert!(expansion.routed_provider_sources.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_expand_aggregate_routes_rejects_nested_aggregate() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 目标自身也是聚合供应商 → 禁止嵌套，丢弃
        let inner = make_aggregate_provider("inner", single_fable_route("ghost", "k3"));
        db.save_provider("claude", &inner).unwrap();

        let agg = make_aggregate_provider("agg", single_fable_route("inner", "k3"));

        let router = ProviderRouter::new(db.clone());
        let expansion = router
            .expand_aggregate_routes(vec![agg], Some(ClaudeTier::Fable), "claude", false)
            .await;

        assert!(expansion.providers.is_empty());
        assert!(expansion.routed_provider_sources.is_empty());
    }

    #[tokio::test]
    #[serial]
    async fn test_expand_aggregate_routes_keeps_plain_providers_and_dedupes() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let kimi = Provider::with_id("kimi".to_string(), "Kimi".to_string(), json!({}), None);
        db.save_provider("claude", &kimi).unwrap();

        let plain = Provider::with_id("plain".to_string(), "Plain".to_string(), json!({}), None);
        let agg = make_aggregate_provider("agg", single_fable_route("kimi", "k3"));

        let router = ProviderRouter::new(db.clone());
        // 链：[普通 provider, 聚合(→kimi), kimi] → kimi 只保留首次出现（来自路由合成）
        let expansion = router
            .expand_aggregate_routes(
                vec![plain, agg, kimi],
                Some(ClaudeTier::Fable),
                "claude",
                false,
            )
            .await;

        let ids: Vec<&str> = expansion.providers.iter().map(|p| p.id.as_str()).collect();
        assert_eq!(ids, ["plain", "kimi"]);
        assert!(expansion.routed_provider_sources.contains_key("kimi"));
        assert!(!expansion.routed_provider_sources.contains_key("plain"));
    }

    #[tokio::test]
    #[serial]
    async fn test_expand_aggregate_routes_skips_circuit_open_target_only_when_checking() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let kimi = Provider::with_id("kimi".to_string(), "Kimi".to_string(), json!({}), None);
        db.save_provider("claude", &kimi).unwrap();

        let router = ProviderRouter::new(db.clone());

        // 连续失败触发熔断（claude 默认 circuit_failure_threshold = 8）
        for _ in 0..8 {
            router
                .record_result("kimi", "claude", false, false, Some("fail".to_string()))
                .await
                .unwrap();
        }
        let breaker = router.get_or_create_circuit_breaker("claude:kimi").await;
        assert!(!breaker.is_available().await);

        // check_breaker = true（故障转移开启）：熔断目标被丢弃
        let expansion = router
            .expand_aggregate_routes(
                vec![make_aggregate_provider(
                    "agg",
                    single_fable_route("kimi", "k3"),
                )],
                Some(ClaudeTier::Fable),
                "claude",
                true,
            )
            .await;
        assert!(expansion.providers.is_empty());

        // check_breaker = false（故障转移关闭）：不查熔断，目标保留
        let expansion = router
            .expand_aggregate_routes(
                vec![make_aggregate_provider(
                    "agg",
                    single_fable_route("kimi", "k3"),
                )],
                Some(ClaudeTier::Fable),
                "claude",
                false,
            )
            .await;
        assert_eq!(expansion.providers.len(), 1);
        assert_eq!(expansion.providers[0].id, "kimi");
    }
}
