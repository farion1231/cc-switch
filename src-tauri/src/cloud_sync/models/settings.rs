use chrono::{DateTime, Utc};
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

impl CloudSyncSettings {
    pub fn new(
        github_token: String,
        gist_url: Option<String>,
        password_hash: String,
        salt: Vec<u8>,
    ) -> Self {
        Self {
            enabled: true,
            github_token: Some(github_token),
            gist_url,
            encryption_password_hash: Some(password_hash),
            salt: Some(salt),
            last_sync_timestamp: None,
            sync_mode: SyncMode::Manual,
            sync_interval_minutes: 15,
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.enabled {
            if self.github_token.as_ref().map(|t| t.is_empty()).unwrap_or(true) {
                return Err("GitHub token cannot be empty when sync is enabled".to_string());
            }

            if let Some(ref url) = self.gist_url {
                if !url.starts_with("https://gist.github.com/") {
                    return Err("Invalid Gist URL format".to_string());
                }
            }

            if let Some(ref salt) = self.salt {
                if salt.len() != 32 {
                    return Err("Salt must be exactly 32 bytes".to_string());
                }
            }
        }

        Ok(())
    }

    pub fn update_last_sync(&mut self) {
        self.last_sync_timestamp = Some(Utc::now());
    }
}