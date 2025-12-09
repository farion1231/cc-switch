//! 供应商路由器模块
//!
//! 负责选择和管理代理目标供应商，实现智能故障转移

use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::circuit_breaker::CircuitBreaker;
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
    /// 返回按优先级排序的可用供应商列表
    pub async fn select_providers(&self, app_type: &str) -> Result<Vec<Provider>, AppError> {
        // 1. 获取所有启用代理的供应商
        let providers = self.db.get_proxy_targets(app_type).await?;

        if providers.is_empty() {
            return Err(AppError::Config(
                "No proxy target providers configured".to_string(),
            ));
        }

        log::debug!(
            "Found {} proxy target providers for app_type: {}",
            providers.len(),
            app_type
        );

        // 2. 按 sort_index 排序（已经在数据库查询中排序了）
        let sorted_providers: Vec<_> = providers.into_values().collect();

        // 3. 过滤可用的供应商（检查熔断器状态）
        let mut available_providers = Vec::new();

        for provider in sorted_providers {
            let circuit_key = format!("{}:{}", app_type, provider.id);
            let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

            if breaker.allow_request().await {
                log::debug!(
                    "Provider {} is available (circuit state: {:?})",
                    provider.id,
                    breaker.get_state().await
                );
                available_providers.push(provider);
            } else {
                log::warn!(
                    "Provider {} is unavailable (circuit breaker open)",
                    provider.id
                );
            }
        }

        if available_providers.is_empty() {
            return Err(AppError::Config(
                "All proxy target providers are unavailable (circuit breakers open)".to_string(),
            ));
        }

        log::info!(
            "Selected {} available providers for failover chain",
            available_providers.len()
        );

        Ok(available_providers)
    }

    /// 记录供应商请求结果
    pub async fn record_result(
        &self,
        provider_id: &str,
        app_type: &str,
        success: bool,
        error_msg: Option<String>,
    ) -> Result<(), AppError> {
        // 1. 更新熔断器状态
        let circuit_key = format!("{app_type}:{provider_id}");
        let breaker = self.get_or_create_circuit_breaker(&circuit_key).await;

        if success {
            breaker.record_success().await;
            log::debug!("Provider {provider_id} request succeeded");
        } else {
            breaker.record_failure().await;
            log::warn!(
                "Provider {} request failed: {}",
                provider_id,
                error_msg.as_deref().unwrap_or("Unknown error")
            );
        }

        // 2. 更新数据库健康状态
        self.db
            .update_provider_health(provider_id, app_type, success, error_msg.clone())
            .await?;

        // 3. 如果连续失败达到熔断阈值，自动禁用代理目标
        if !success {
            let health = self.db.get_provider_health(provider_id, app_type).await?;

            // 获取熔断器配置
            let config = self.db.get_circuit_breaker_config().await.ok();
            let failure_threshold = config.map(|c| c.failure_threshold).unwrap_or(5);

            // 如果连续失败达到阈值，自动关闭该供应商的代理开关
            if health.consecutive_failures >= failure_threshold {
                log::warn!(
                    "Provider {} has failed {} times (threshold: {}), auto-disabling proxy target",
                    provider_id,
                    health.consecutive_failures,
                    failure_threshold
                );
                self.db
                    .set_proxy_target(provider_id, app_type, false)
                    .await?;
            }
        }

        Ok(())
    }

    /// 重置熔断器（手动恢复）
    #[allow(dead_code)]
    pub async fn reset_circuit_breaker(&self, circuit_key: &str) {
        let breakers = self.circuit_breakers.read().await;
        if let Some(breaker) = breakers.get(circuit_key) {
            log::info!("Manually resetting circuit breaker for {circuit_key}");
            breaker.reset().await;
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

    #[tokio::test]
    async fn test_provider_router_creation() {
        let db = Arc::new(Database::new_in_memory().unwrap());
        let router = ProviderRouter::new(db);

        // 测试创建熔断器
        let breaker = router.get_or_create_circuit_breaker("claude:test").await;
        assert!(breaker.allow_request().await);
    }
}
