//! Application state

use std::sync::Arc;

use crate::database::Database;
use crate::services::ProxyService;

/// Application state shared across the app
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
    pub proxy_service: ProxyService,
}

impl AppState {
    pub fn new(db: Database) -> Self {
        let db = Arc::new(db);
        let proxy_service = ProxyService::new(db.clone());
        Self { db, proxy_service }
    }
}
