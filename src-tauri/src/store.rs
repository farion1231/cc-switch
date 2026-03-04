use crate::database::Database;
use crate::services::GuardianService;
use crate::services::ProxyService;
use std::sync::Arc;

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub guardian_service: GuardianService,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());
        let guardian_service = GuardianService::new(db.clone(), proxy_service.clone());

        Self {
            db,
            proxy_service,
            guardian_service,
        }
    }
}
