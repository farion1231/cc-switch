use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use crate::cloud_sync::models::BackupReason;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigurationBackup {
    pub backup_id: String,
    pub created_at: DateTime<Utc>,
    pub original_config: String,
    pub backup_reason: BackupReason,
    pub file_path: PathBuf,
    pub checksum: String,
}

impl ConfigurationBackup {
    pub fn new(
        backup_id: String,
        original_config: String,
        backup_reason: BackupReason,
        file_path: PathBuf,
    ) -> Self {
        use sha2::{Sha256, Digest};

        let mut hasher = Sha256::new();
        hasher.update(original_config.as_bytes());
        let checksum = format!("{:x}", hasher.finalize());

        Self {
            backup_id,
            created_at: Utc::now(),
            original_config,
            backup_reason,
            file_path,
            checksum,
        }
    }

    #[allow(dead_code)]
    pub fn validate_checksum(&self) -> bool {
        use sha2::{Sha256, Digest};

        let mut hasher = Sha256::new();
        hasher.update(self.original_config.as_bytes());
        let calculated = format!("{:x}", hasher.finalize());

        calculated == self.checksum
    }
}