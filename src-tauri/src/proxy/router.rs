//! Provider路由器
//!
//! 负责选择合适的Provider进行请求转发，支持健康检查和故障转移

use super::ProxyError;
use crate::{app_config::AppType, database::Database, provider::Provider};
use std::sync::Arc;

pub struct ProviderRouter {
    db: Arc<Database>,
}

impl ProviderRouter {
    pub fn new(db: Arc<Database>) -> Self {
        Self { db }
    }

    /// 选择Provider（带故障转移）
    ///
    /// 优先使用当前Provider，失败则尝试备用Provider
    pub async fn select_provider(
        &self,
        app_type: &AppType,
        failed_ids: &[String],
    ) -> Result<Provider, ProxyError> {
        // 1. 尝试获取当前Provider
        match self.get_current_provider(app_type, failed_ids).await {
            Ok(provider) => return Ok(provider),
            Err(e) => {
                log::debug!("当前Provider不可用: {e:?}");
            }
        }

        // 2. 尝试备用Provider
        self.select_fallback(app_type, failed_ids).await
    }

    /// 获取当前Provider
    async fn get_current_provider(
        &self,
        app_type: &AppType,
        failed_ids: &[String],
    ) -> Result<Provider, ProxyError> {
        // 1. 尝试获取 Proxy Target Provider ID
        let proxy_target_id = self
            .db
            .get_proxy_target_provider(app_type.as_str())
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        // 2. 获取 Current Provider ID (作为 fallback)
        let current_id = self
            .db
            .get_current_provider(app_type.as_str())
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        // 3. 确定使用的 ID (优先 proxy_target)
        let target_id = proxy_target_id
            .or(current_id)
            .ok_or(ProxyError::NoAvailableProvider)?;

        // 4. 获取所有Provider
        let providers = self
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        // 5. 找到目标Provider
        let target = providers
            .get(&target_id)
            .ok_or(ProxyError::NoAvailableProvider)?;

        // 4. 检查是否在失败列表中
        if failed_ids.contains(&target.id) {
            return Err(ProxyError::ProviderUnhealthy("Provider已失败".to_string()));
        }

        // 5. 检查健康状态
        if self.is_provider_healthy(target, app_type).await {
            Ok(target.clone())
        } else {
            Err(ProxyError::ProviderUnhealthy(target.id.clone()))
        }
    }

    /// 选择备用Provider
    async fn select_fallback(
        &self,
        app_type: &AppType,
        failed_ids: &[String],
    ) -> Result<Provider, ProxyError> {
        let providers = self
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        // 过滤失败的Provider，按sort_index排序
        let mut available: Vec<_> = providers
            .into_values()
            .filter(|p| !failed_ids.contains(&p.id))
            .collect();

        available.sort_by_key(|p| p.sort_index.unwrap_or(9999));

        // 寻找健康的Provider
        for provider in available {
            if self.is_provider_healthy(&provider, app_type).await {
                log::info!("选择备用Provider: {}", provider.name);
                return Ok(provider);
            }
        }

        log::warn!("无可用Provider");
        Err(ProxyError::NoAvailableProvider)
    }

    /// 检查Provider是否健康
    async fn is_provider_healthy(&self, provider: &Provider, app_type: &AppType) -> bool {
        // 从数据库查询健康状态
        match self
            .db
            .get_provider_health(&provider.id, app_type.as_str())
            .await
        {
            Ok(health) => {
                // 连续失败3次以上视为不健康
                health.is_healthy && health.consecutive_failures < 3
            }
            Err(_) => {
                // 未记录状态时默认健康
                true
            }
        }
    }

    /// 更新Provider健康状态
    pub async fn update_health(
        &self,
        provider: &Provider,
        app_type: &AppType,
        success: bool,
        error_msg: Option<String>,
    ) {
        if let Err(e) = self
            .db
            .update_provider_health(&provider.id, app_type.as_str(), success, error_msg)
            .await
        {
            log::warn!("更新Provider健康状态失败: {e:?}");
        }
    }
}
