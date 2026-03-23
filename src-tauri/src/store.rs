use crate::claude_notify::ClaudeNotifyService;
use crate::database::Database;
use crate::services::ProxyService;
use std::sync::Arc;

/// 全局应用状态
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
    pub claude_notify_service: ClaudeNotifyService,
}

impl AppState {
    /// 创建新的应用状态
    pub fn new(db: Arc<Database>) -> Self {
        let proxy_service = ProxyService::new(db.clone());
        let claude_notify_service = ClaudeNotifyService::new();

        Self {
            db,
            proxy_service,
            claude_notify_service,
        }
    }
}
