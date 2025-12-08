//! Provider路由器
//!
//! 负责选择合适的Provider进行请求转发

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

    /// 选择Provider（只使用标记为代理目标的 Provider）
    pub async fn select_provider(
        &self,
        app_type: &AppType,
        _failed_ids: &[String],
    ) -> Result<Provider, ProxyError> {
        // 1. 获取 Proxy Target Provider ID
        let proxy_target_id = self
            .db
            .get_proxy_target_provider(app_type.as_str())
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        let target_id = proxy_target_id.ok_or_else(|| {
            log::warn!("[{}] 未设置代理目标 Provider", app_type.as_str());
            ProxyError::NoAvailableProvider
        })?;

        // 2. 获取所有 Provider
        let providers = self
            .db
            .get_all_providers(app_type.as_str())
            .map_err(|e| ProxyError::DatabaseError(e.to_string()))?;

        // 3. 找到目标 Provider
        let target = providers.get(&target_id).ok_or_else(|| {
            log::warn!(
                "[{}] 代理目标 Provider 不存在: {}",
                app_type.as_str(),
                target_id
            );
            ProxyError::NoAvailableProvider
        })?;

        log::info!(
            "[{}] 使用代理目标 Provider: {}",
            app_type.as_str(),
            target.name
        );
        Ok(target.clone())
    }

    /// 更新Provider健康状态（保留接口但不影响选择）
    #[allow(dead_code)]
    pub async fn update_health(
        &self,
        _provider: &Provider,
        _app_type: &AppType,
        _success: bool,
        _error_msg: Option<String>,
    ) {
        // 不再记录健康状态
    }
}
