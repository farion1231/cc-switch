//! Application state

use std::sync::Arc;

use crate::database::Database;

/// Application state shared across the app
#[derive(Clone)]
pub struct AppState {
    pub db: Arc<Database>,
}

impl AppState {
    pub fn new(db: Database) -> Self {
        Self { db: Arc::new(db) }
    }
}
