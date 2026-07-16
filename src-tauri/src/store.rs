use crate::database::Database;
use crate::services::{ProviderMutationCoordinator, ProxyService, UsageCache};
use std::sync::Arc;

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub usage_cache: Arc<UsageCache>,
    pub provider_mutation_coordinator: ProviderMutationCoordinator,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());
        let provider_mutation_coordinator = ProviderMutationCoordinator::new(db.clone());

        Self {
            db,
            proxy_service,
            usage_cache: Arc::new(UsageCache::new()),
            provider_mutation_coordinator,
        }
    }
}
