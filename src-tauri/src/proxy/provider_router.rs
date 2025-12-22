//! 供应商路由器模块
//!
//! 负责选择和管理代理目标供应商，实现智能故障转移

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::{AllowResult, CircuitBreaker, CircuitBreakerConfig};
use std::collections::HashMap;
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
    /// - 故障转移开启时：完全按照故障转移队列顺序返回，忽略当前供应商设置
    pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        let mut result = Vec::new();

        // 检查该应用的自动故障转移开关是否开启
        let failover_key = format!("auto_failover_enabled_{app_type}");
        let auto_failover_enabled = match self.db.get_setting(&failover_key) {
            Ok(Some(value)) => {
                let enabled = value == "true";
                log::info!(
                    "[{app_type}] Failover setting '{failover_key}' = '{value}', enabled: {enabled}"
                );
                enabled
            }
            Ok(None) => {
                log::warn!(
                    "[{app_type}] Failover setting '{failover_key}' not found in database, defaulting to disabled"
                );
                false
            }
            Err(e) => {
                log::error!(
                    "[{app_type}] Failed to read failover setting '{failover_key}': {e}, defaulting to disabled"
                );
                false
            }
        };

        if auto_failover_enabled {
            // 故障转移开启：使用 in_failover_queue 标记的供应商，按 sort_index 排序
            let failover_providers = self.db.get_failover_providers(app_type)?;
            log::info!(
                "[{}] Failover enabled, using queue order ({} items)",
                app_type,
                failover_providers.len()
            );

            for provider in failover_providers {
                // 检查熔断器状态
                let circuit_key = format!("{}:{}", app_type, provider.id);
                let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

                if breaker.is_available().await {
                    log::info!(
                        "[{}] Queue provider available: {} ({}) at sort_index {:?}",
                        app_type,
                        provider.name,
                        provider.id,
                        provider.sort_index
                    );
                    result.push(provider);
                } else {
                    log::debug!(
                        "[{}] Queue provider {} circuit breaker open, skipping",
                        app_type,
                        provider.name
                    );
                }
            }
        } else {
            // 故障转移关闭：仅使用当前供应商
            log::info!("[{app_type}] Failover disabled, using current provider only");

            if let Some(current_id) = self.db.get_current_provider(app_type)? {
                if let Some(current) = self.db.get_provider_by_id(&current_id, app_type)? {
                    let circuit_key = format!("{}:{}", app_type, current.id);
                    let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

                    if breaker.is_available().await {
                        log::info!(
                            "[{}] Current provider available: {} ({})",
                            app_type,
                            current.name,
                            current.id
                        );
                        result.push(current);
                    } else {
                        log::warn!(
                            "[{}] Current provider {} circuit breaker open",
                            app_type,
                            current.name
                        );
                    }
                }
            }
        }

        if result.is_empty() {
            return Err(AppError::Config(format!(
                "No available provider for {app_type} (all circuit breakers open or no providers configured)"
            )));
        }

        log::info!(
            "[{}] Provider chain: {} provider(s) available",
            app_type,
            result.len()
        );

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
        // 1. 获取熔断器配置（用于更新健康状态和判断是否禁用）
        let config = self.db.get_circuit_breaker_config().await.ok();
        let failure_threshold = config.map(|c| c.failure_threshold).unwrap_or(5);

        // 2. 更新熔断器状态
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

        if success {
            breaker.record_success(used_half_open_permit).await;
            log::debug!("Provider {provider_id} request succeeded");
        } else {
            breaker.record_failure(used_half_open_permit).await;
            log::warn!(
                "Provider {} request failed: {}",
                provider_id,
                error_msg.as_deref().unwrap_or("Unknown error")
            );
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
            log::info!("Manually resetting circuit breaker for {circuit_key}");
            breaker.reset().await;
        }
    }

    /// 重置指定供应商的熔断器
    pub async fn reset_provider_breaker(&self, provider_id: &str, app_type: &str) {
        let circuit_key = format!("{app_type}:{provider_id}");
        self.reset_circuit_breaker(&circuit_key).await;
    }

    /// 更新所有熔断器的配置（热更新）
    ///
    /// 当用户在 UI 中修改熔断器配置后调用此方法，
    /// 所有现有的熔断器会立即使用新配置
    pub async fn update_all_configs(&self, config: CircuitBreakerConfig) {
        let breakers = self.circuit_breakers.read().await;
        let count = breakers.len();

        for breaker in breakers.values() {
            breaker.update_config(config.clone()).await;
        }

        log::info!("已更新 {count} 个熔断器的配置");
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

        // 从数据库加载配置
        let config = self
            .db
            .get_circuit_breaker_config()
            .await
            .unwrap_or_default();

        log::debug!("Creating new circuit breaker for {key} with config: {config:?}");

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

    #[tokio::test]
    async fn test_provider_router_creation() {
        let db = Arc::new(Database::memory().unwrap());
        let router = ProviderRouter::new(db);

        let breaker = router.get_or_create_circuit_breaker("claude:test").await;
        assert!(breaker.allow_request().await.allowed);
    }

    #[tokio::test]
    async fn test_failover_disabled_uses_current_provider() {
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
    async fn test_failover_enabled_uses_queue_order() {
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
        db.set_setting("auto_failover_enabled_claude", "true")
            .unwrap();

        let router = ProviderRouter::new(db.clone());
        let providers = router.select_providers("claude").await.unwrap();

        assert_eq!(providers.len(), 2);
        // 按 sort_index 排序：b(1) 在前，a(2) 在后
        assert_eq!(providers[0].id, "b");
        assert_eq!(providers[1].id, "a");
    }

    #[tokio::test]
    async fn test_select_providers_does_not_consume_half_open_permit() {
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
        db.set_setting("auto_failover_enabled_claude", "true")
            .unwrap();

        let router = ProviderRouter::new(db.clone());

        router
            .record_result("b", "claude", false, false, Some("fail".to_string()))
            .await
            .unwrap();

        let providers = router.select_providers("claude").await.unwrap();
        assert_eq!(providers.len(), 2);

        assert!(router.allow_provider_request("b", "claude").await.allowed);
    }
}
