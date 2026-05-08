//! 供应商路由器模块
//!
//! 负责选择和管理代理目标供应商，实现智能故障转移

use crate::app_config::AppType;
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::{AllowResult, CircuitBreaker, CircuitBreakerConfig};
use crate::proxy::types::RequestType;
use std::collections::HashMap;
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

    /// 根据请求类型选择供应商（智能路由）
    ///
    /// 当智能路由启用时，根据请求类型选择对应的供应商队列：
    /// - Main: 使用 main_request_queue
    /// - Others: 使用 others_request_queue
    ///
    /// 如果对应队列为空，回退到默认的 select_providers 行为。
    pub async fn select_providers_for_request(
        &self,
        app_type: &str,
        request_type: RequestType,
    ) -> Result<Vec<Provider>, AppError> {
        let config = self.db.get_proxy_config_for_app(app_type).await
            .map_err(|e| AppError::Database(e.to_string()))?;

        if !config.smart_routing_enabled {
            return self.select_providers(app_type).await;
        }

        let queue = match request_type {
            RequestType::Main => &config.main_request_queue,
            RequestType::Others => &config.others_request_queue,
        };

        if queue.is_empty() {
            log::debug!(
                "[{app_type}] Smart routing queue for {:?} is empty, falling back to default",
                request_type
            );
            return self.select_providers(app_type).await;
        }

        log::info!(
            "[{app_type}] Smart routing: {:?} request using queue with {} providers",
            request_type,
            queue.len()
        );

        self.select_from_queue(app_type, queue).await
    }

    /// 从指定的供应商 ID 队列中选择可用供应商
    async fn select_from_queue(
        &self,
        app_type: &str,
        queue: &[String],
    ) -> Result<Vec<Provider>, AppError> {
        if queue.is_empty() {
            log::warn!("[{app_type}] Smart routing: queue is empty (0 providers) — falling back to default");
            return self.select_providers(app_type).await;
        }

        let mut skipped_not_found = 0usize;
        let mut skipped_circuit_open = 0usize;

        // 一次性读取配置，避免循环中重复查询数据库
        let auto_failover_enabled = self
            .db
            .get_proxy_config_for_app(app_type)
            .await
            .map(|cfg| cfg.auto_failover_enabled)
            .unwrap_or(false);

        // 使用 spawn_blocking 避免同步 DB 调用阻塞 tokio worker 线程
        let app_type_owned = app_type.to_string();
        let db = self.db.clone();
        let all_providers = tokio::task::spawn_blocking(move || {
            db.get_all_providers(&app_type_owned)
        })
        .await
        .map_err(|e| AppError::Database(format!("spawn_blocking failed: {e}")))?
        .map_err(|e| AppError::Database(e.to_string()))?;

        // auto_failover_enabled = false 时：只返回队列中第一个可用的 provider
        // auto_failover_enabled = true 时：返回队列中所有可用的 provider（支持故障转移）
        //
        // 注意：熔断器检查始终执行，即使 auto_failover_enabled = false。
        // 这是为了避免向已知不可用的 provider 发送请求，提高首次请求成功率。
        // auto_failover_enabled = false 只控制是否返回多个 provider（用于故障转移链），
        // 不应影响熔断器过滤逻辑。
        let target_count = if auto_failover_enabled { queue.len() } else { 1 };

        let mut result = Vec::new();

        for provider_id in queue {
            if result.len() >= target_count {
                break;
            }

            let Some(provider) = all_providers.get(provider_id).cloned() else {
                log::warn!("[{app_type}] Smart routing: Provider {provider_id} not found, skipping");
                skipped_not_found += 1;
                continue;
            };

            // 熔断器检查始终执行（不受 auto_failover_enabled 影响）
            let circuit_key = format!("{app_type}:{}", provider.id);
            let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;
            if !breaker.is_available().await {
                log::debug!(
                    "[{app_type}] Smart routing: Provider {} is circuit-open, skipping",
                    provider.name
                );
                skipped_circuit_open += 1;
                continue;
            }

            result.push(provider);
        }

        if result.is_empty() {
            if skipped_not_found > 0 || skipped_circuit_open > 0 {
                log::warn!(
                    "[{app_type}] Smart routing: queue has {} providers, {} not found, {} circuit-open — falling back to default",
                    queue.len(),
                    skipped_not_found,
                    skipped_circuit_open
                );
            } else {
                log::warn!(
                    "[{app_type}] Smart routing: queue is empty (0 providers) — falling back to default"
                );
            }
            return self.select_providers(app_type).await;
        }

        log::debug!(
            "[{app_type}] Smart routing: selected {} available providers from queue (target: {}, {} not-found, {} circuit-open, auto_failover: {})",
            result.len(),
            target_count,
            skipped_not_found,
            skipped_circuit_open,
            auto_failover_enabled
        );

        Ok(result)
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

    // ==================== 智能路由测试 ====================

    /// 真实场景测试：智能路由 + auto_failover 关闭时，只返回队列中第一个 provider
    ///
    /// 场景描述：
    /// - 用户在主界面选择了 Provider A（glm5.1）作为当前供应商
    /// - 在智能路由设置中，将 Main 队列配置为 [Provider A, Provider B]
    /// - 用户关闭了自动故障转移（auto_failover_enabled = false）
    /// - 期望：主对话只使用 Provider A，不会 failover 到 Provider B
    #[tokio::test]
    #[serial]
    async fn test_smart_routing_auto_failover_disabled_returns_only_first_provider() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 设置两个 Provider：glm5.1 和 minimax
        let glm51 = Provider::with_id(
            "glm5.1".to_string(),
            "GLM-5.1".to_string(),
            json!({}),
            None,
        );
        let minimax = Provider::with_id(
            "minimax".to_string(),
            "MiniMax".to_string(),
            json!({}),
            None,
        );
        db.save_provider("claude", &glm51).unwrap();
        db.save_provider("claude", &minimax).unwrap();

        // 主界面选择的供应商是 glm5.1
        db.set_current_provider("claude", "glm5.1").unwrap();

        // 智能路由配置：Main 队列 = [glm5.1, minimax]，auto_failover = false
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.auto_failover_enabled = false;
        config.main_request_queue = vec!["glm5.1".to_string(), "minimax".to_string()];
        config.others_request_queue = vec!["minimax".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 主对话请求：只应返回 glm5.1，不应返回 minimax
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();

        assert_eq!(
            providers.len(),
            1,
            "auto_failover=false 时，智能路由应只返回第一个 provider"
        );
        assert_eq!(
            providers[0].id, "glm5.1",
            "主对话应使用 glm5.1，不应 failover 到 minimax"
        );
    }

    /// 真实场景测试：智能路由 + auto_failover 开启时，返回队列中所有可用的 provider
    ///
    /// 场景描述：
    /// - 用户在智能路由设置中，将 Main 队列配置为 [Provider A, Provider B]
    /// - 用户开启了自动故障转移（auto_failover_enabled = true）
    /// - 期望：主对话可以使用 Provider A 或 Provider B（故障转移）
    #[tokio::test]
    #[serial]
    async fn test_smart_routing_auto_failover_enabled_returns_all_available_providers() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 设置两个 Provider
        let glm51 = Provider::with_id(
            "glm5.1".to_string(),
            "GLM-5.1".to_string(),
            json!({}),
            None,
        );
        let minimax = Provider::with_id(
            "minimax".to_string(),
            "MiniMax".to_string(),
            json!({}),
            None,
        );
        db.save_provider("claude", &glm51).unwrap();
        db.save_provider("claude", &minimax).unwrap();

        // 智能路由配置：Main 队列 = [glm5.1, minimax]，auto_failover = true
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.auto_failover_enabled = true;
        config.main_request_queue = vec!["glm5.1".to_string(), "minimax".to_string()];
        config.others_request_queue = vec!["minimax".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 主对话请求：应返回所有可用的 provider（支持故障转移）
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();

        assert_eq!(
            providers.len(),
            2,
            "auto_failover=true 时，智能路由应返回队列中所有可用的 provider"
        );
        assert_eq!(providers[0].id, "glm5.1", "队列第一个应是 glm5.1");
        assert_eq!(providers[1].id, "minimax", "队列第二个应是 minimax");
    }

    /// 真实场景测试：智能路由 + auto_failover 关闭 + 第一个 provider 不可用时
    ///
    /// 场景描述：
    /// - Main 队列 = [Provider A, Provider B]
    /// - auto_failover_enabled = false
    /// - Provider A 已熔断
    /// - 期望：返回 Provider B（跳过不可用的第一个）
    #[tokio::test]
    #[serial]
    async fn test_smart_routing_auto_failover_disabled_skips_unavailable_first() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 配置熔断器：1 次失败即熔断
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 999999,
            ..Default::default()
        })
        .await
        .unwrap();

        let glm51 = Provider::with_id(
            "glm5.1".to_string(),
            "GLM-5.1".to_string(),
            json!({}),
            None,
        );
        let minimax = Provider::with_id(
            "minimax".to_string(),
            "MiniMax".to_string(),
            json!({}),
            None,
        );
        db.save_provider("claude", &glm51).unwrap();
        db.save_provider("claude", &minimax).unwrap();

        // 智能路由配置：Main 队列 = [glm5.1, minimax]，auto_failover = false
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.auto_failover_enabled = false;
        config.main_request_queue = vec!["glm5.1".to_string(), "minimax".to_string()];
        config.others_request_queue = vec!["minimax".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 让 glm5.1 熔断
        router
            .record_result("glm5.1", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        // 主对话请求：glm5.1 已熔断，auto_failover=false，但仍应跳过不可用的并返回 minimax
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();

        assert_eq!(
            providers.len(),
            1,
            "即使 auto_failover=false，也应跳过不可用的 provider 并返回后续可用的"
        );
        assert_eq!(
            providers[0].id, "minimax",
            "glm5.1 不可用时应使用 minimax"
        );
    }

    /// 真实场景测试：智能路由 + auto_failover 关闭 + 所有 provider 都不可用时 fallback
    ///
    /// 场景描述：
    /// - Main 队列 = [Provider A, Provider B]
    /// - auto_failover_enabled = false
    /// - Provider A 和 Provider B 都已熔断
    /// - 期望：fallback 到默认行为（返回 current provider）
    #[tokio::test]
    #[serial]
    async fn test_smart_routing_auto_failover_disabled_all_unavailable_falls_back() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 配置熔断器：1 次失败即熔断
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 999999,
            ..Default::default()
        })
        .await
        .unwrap();

        let glm51 = Provider::with_id(
            "glm5.1".to_string(),
            "GLM-5.1".to_string(),
            json!({}),
            None,
        );
        let minimax = Provider::with_id(
            "minimax".to_string(),
            "MiniMax".to_string(),
            json!({}),
            None,
        );
        let deepseek = Provider::with_id(
            "deepseek".to_string(),
            "DeepSeek".to_string(),
            json!({}),
            None,
        );
        db.save_provider("claude", &glm51).unwrap();
        db.save_provider("claude", &minimax).unwrap();
        db.save_provider("claude", &deepseek).unwrap();

        // 设置 current provider 为 deepseek
        db.set_current_provider("claude", "deepseek").unwrap();

        // 智能路由配置：Main 队列 = [glm5.1, minimax]，auto_failover = false
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.auto_failover_enabled = false;
        config.main_request_queue = vec!["glm5.1".to_string(), "minimax".to_string()];
        config.others_request_queue = vec!["minimax".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 让 glm5.1 和 minimax 都熔断
        router
            .record_result("glm5.1", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();
        router
            .record_result("minimax", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        // 主对话请求：队列中所有 provider 都不可用，fallback 到 current provider
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();

        assert_eq!(
            providers.len(),
            1,
            "所有队列 provider 都不可用时应 fallback"
        );
        assert_eq!(
            providers[0].id, "deepseek",
            "fallback 应使用 current provider"
        );
    }

    /// 真实场景测试：智能路由中子代理请求的行为
    ///
    /// 场景描述：
    /// - Main 队列 = [glm5.1]，Others 队列 = [minimax]
    /// - auto_failover_enabled = false
    /// - 主对话使用 glm5.1，子代理使用 minimax
    #[tokio::test]
    #[serial]
    async fn test_smart_routing_others_queue_respects_auto_failover_setting() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let glm51 = Provider::with_id(
            "glm5.1".to_string(),
            "GLM-5.1".to_string(),
            json!({}),
            None,
        );
        let minimax = Provider::with_id(
            "minimax".to_string(),
            "MiniMax".to_string(),
            json!({}),
            None,
        );
        db.save_provider("claude", &glm51).unwrap();
        db.save_provider("claude", &minimax).unwrap();

        // 智能路由配置：Main = [glm5.1]，Others = [minimax, glm5.1]，auto_failover = false
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.auto_failover_enabled = false;
        config.main_request_queue = vec!["glm5.1".to_string()];
        config.others_request_queue = vec!["minimax".to_string(), "glm5.1".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // 子代理请求：只应返回 Others 队列中的第一个（minimax）
        let providers = router
            .select_providers_for_request("claude", RequestType::Others)
            .await
            .unwrap();

        assert_eq!(
            providers.len(),
            1,
            "auto_failover=false 时，Others 队列也应只返回第一个"
        );
        assert_eq!(
            providers[0].id, "minimax",
            "子代理请求应使用 minimax"
        );
    }

    #[tokio::test]
    #[serial]
    async fn test_smart_routing_disabled_falls_back_to_default() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();

        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_smart_routing_main_queue_selection() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.save_provider("claude", &provider_b).unwrap();

        // 配置智能路由：Main 队列 = [b], Others 队列 = [a]
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.main_request_queue = vec!["b".to_string()];
        config.others_request_queue = vec!["a".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());

        // Main 请求 → 使用 main_request_queue → 返回 b
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "b");

        // Others 请求 → 使用 others_request_queue → 返回 a
        let providers = router
            .select_providers_for_request("claude", RequestType::Others)
            .await
            .unwrap();
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_smart_routing_empty_queue_falls_back() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_a =
            Provider::with_id("a".to_string(), "Provider A".to_string(), json!({}), None);
        db.save_provider("claude", &provider_a).unwrap();
        db.set_current_provider("claude", "a").unwrap();

        // 启用智能路由但队列为空
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.main_request_queue = vec![];
        config.others_request_queue = vec![];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();

        // 空队列 → 回退到默认 select_providers → 返回 current provider
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "a");
    }

    #[tokio::test]
    #[serial]
    async fn test_smart_routing_provider_not_found_skipped() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        let provider_b =
            Provider::with_id("b".to_string(), "Provider B".to_string(), json!({}), None);
        db.save_provider("claude", &provider_b).unwrap();

        // 智能路由队列包含不存在的 provider "x"
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.main_request_queue = vec!["x".to_string(), "b".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router
            .select_providers_for_request("claude", RequestType::Main)
            .await
            .unwrap();

        // 跳过不存在的 x，返回 b
        assert_eq!(providers.len(), 1);
        assert_eq!(providers[0].id, "b");
    }

    #[tokio::test]
    #[serial]
    async fn test_smart_routing_all_circuit_broken_falls_back() {
        let _home = TempHome::new();
        let db = Arc::new(Database::memory().unwrap());

        // 配置熔断器：1 次失败即熔断，超时很长（不会自动恢复）
        db.update_circuit_breaker_config(&CircuitBreakerConfig {
            failure_threshold: 1,
            timeout_seconds: 999999,
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
        db.set_current_provider("claude", "a").unwrap();

        // 智能路由：Others 队列 = [b]，但 b 将熔断
        let mut config = db.get_proxy_config_for_app("claude").await.unwrap();
        config.smart_routing_enabled = true;
        config.auto_failover_enabled = true;
        config.others_request_queue = vec!["b".to_string()];
        db.update_proxy_config_for_app(config).await.unwrap();

        // 把 a 加入故障转移队列作为回退
        db.add_to_failover_queue("claude", "a").unwrap();

        let router = ProviderRouter::new(db.clone());

        // 让 b 熔断
        router
            .record_result("b", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        // Others 请求：b 已熔断 → 智能路由队列全部不可用 → 回退到默认故障转移队列 [a]
        let providers = router
            .select_providers_for_request("claude", RequestType::Others)
            .await
            .unwrap();
        assert!(!providers.is_empty());
        // 回退到了故障转移队列的第一个可用 provider
        assert_eq!(providers[0].id, "a");
    }
}
