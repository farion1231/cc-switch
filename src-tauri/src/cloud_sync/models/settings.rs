use chrono::DateTime;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use crate::cloud_sync::models::SyncMode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSyncSettings {
    pub enabled: bool,
    pub github_token: Option<String>,
    pub gist_url: Option<String>,
    pub encryption_password_hash: Option<String>,
    pub salt: Option<Vec<u8>>,
    pub last_sync_timestamp: Option<DateTime<Utc>>,
    pub sync_mode: SyncMode,
    pub sync_interval_minutes: u32,
}

impl Default for CloudSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            github_token: None,
            gist_url: None,
            encryption_password_hash: None,
            salt: None,
            last_sync_timestamp: None,
            sync_mode: SyncMode::Manual,
            sync_interval_minutes: 15,
        }
    }
}