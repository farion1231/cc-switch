use crate::cloud_sync::{CloudSyncResult, CloudSyncError};
use crate::cloud_sync::models::{ConfigurationBackup, BackupReason};
use std::path::{Path, PathBuf};
use std::fs;
use std::io::{Read, Write};
use uuid::Uuid;
use chrono::Utc;

const BACKUP_DIR: &str = ".cc-switch-backups";

pub struct BackupService {
    backup_dir: PathBuf,
}

impl BackupService {
    pub fn new() -> Self {
        let backup_dir = dirs::home_dir()
            .expect("Cannot find home directory")
            .join(BACKUP_DIR);

        Self { backup_dir }
    }

    pub fn create_backup(&self, config_path: &PathBuf) -> CloudSyncResult<String> {
        // Ensure backup directory exists
        fs::create_dir_all(&self.backup_dir)
            .map_err(|e| CloudSyncError::Io(format!("Failed to create backup directory: {}", e)))?;

        // Read original configuration
        let original_config = fs::read_to_string(config_path)
            .map_err(|e| CloudSyncError::Io(format!("Failed to read configuration file: {}", e)))?;

        // Create backup
        let backup_id = Uuid::new_v4().to_string();
        let backup = ConfigurationBackup::new(
            backup_id.clone(),
            original_config.clone(),
            BackupReason::ManualBackup,
            config_path.clone(),
        );

        // Serialize and save backup
        let backup_json = serde_json::to_string_pretty(&backup)
            .map_err(|e| CloudSyncError::Parse(format!("Failed to serialize backup: {}", e)))?;

        let backup_file = self.backup_dir.join(format!("{}.json", backup_id));
        let mut file = fs::File::create(&backup_file)
            .map_err(|e| CloudSyncError::Io(format!("Failed to create backup file: {}", e)))?;

        file.write_all(backup_json.as_bytes())
            .map_err(|e| CloudSyncError::Io(format!("Failed to write backup file: {}", e)))?;

        Ok(backup_id)
    }

    pub fn restore_backup(&self, backup_id: &str) -> CloudSyncResult<()> {
        // Load backup
        let backup_file = self.backup_dir.join(format!("{}.json", backup_id));

        if !backup_file.exists() {
            return Err(CloudSyncError::NotFound(format!("Backup {} not found", backup_id)));
        }

        let backup_json = fs::read_to_string(&backup_file)
            .map_err(|e| CloudSyncError::Io(format!("Failed to read backup file: {}", e)))?;

        let backup: ConfigurationBackup = serde_json::from_str(&backup_json)
            .map_err(|e| CloudSyncError::Parse(format!("Failed to parse backup: {}", e)))?;

        // Validate checksum
        if !backup.validate_checksum() {
            return Err(CloudSyncError::Validation("Backup checksum validation failed".into()));
        }

        // Create a backup of current configuration before restoring
        if backup.file_path.exists() {
            let _current_backup_id = self.create_backup(&backup.file_path)?;
            // Created backup before restoration
        }

        // Restore the configuration
        let mut file = fs::File::create(&backup.file_path)
            .map_err(|e| CloudSyncError::Io(format!("Failed to create configuration file: {}", e)))?;

        file.write_all(backup.original_config.as_bytes())
            .map_err(|e| CloudSyncError::Io(format!("Failed to write configuration file: {}", e)))?;

        Ok(())
    }

    pub fn list_backups(&self) -> CloudSyncResult<Vec<ConfigurationBackup>> {
        if !self.backup_dir.exists() {
            return Ok(Vec::new());
        }

        let mut backups = Vec::new();

        let entries = fs::read_dir(&self.backup_dir)
            .map_err(|e| CloudSyncError::Io(format!("Failed to read backup directory: {}", e)))?;

        for entry in entries {
            let entry = entry.map_err(|e| CloudSyncError::Io(format!("Failed to read directory entry: {}", e)))?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                let backup_json = fs::read_to_string(&path)
                    .map_err(|e| CloudSyncError::Io(format!("Failed to read backup file: {}", e)))?;

                if let Ok(backup) = serde_json::from_str::<ConfigurationBackup>(&backup_json) {
                    backups.push(backup);
                }
            }
        }

        // Sort by creation date (newest first)
        backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(backups)
    }

    pub fn cleanup_old_backups(&self, keep_count: usize) -> CloudSyncResult<()> {
        let backups = self.list_backups()?;

        if backups.len() <= keep_count {
            return Ok(());
        }

        // Remove oldest backups
        for backup in backups.iter().skip(keep_count) {
            let backup_file = self.backup_dir.join(format!("{}.json", backup.backup_id));
            fs::remove_file(&backup_file)
                .map_err(|e| CloudSyncError::Io(format!("Failed to remove old backup: {}", e)))?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_service() -> (BackupService, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let mut service = BackupService::new();
        service.backup_dir = temp_dir.path().join("backups");
        (service, temp_dir)
    }

    #[test]
    fn test_create_and_restore_backup() {
        let (service, temp_dir) = create_test_service();

        // Create test configuration file
        let config_path = temp_dir.path().join("config.json");
        let test_config = r#"{"test": "data"}"#;
        fs::write(&config_path, test_config).unwrap();

        // Create backup
        let backup_id = service.create_backup(&config_path).unwrap();
        assert!(!backup_id.is_empty());

        // Modify original file
        fs::write(&config_path, r#"{"modified": "data"}"#).unwrap();

        // Restore backup
        service.restore_backup(&backup_id).unwrap();

        // Verify restoration
        let restored_content = fs::read_to_string(&config_path).unwrap();
        assert_eq!(restored_content, test_config);
    }

    #[test]
    fn test_list_backups() {
        let (service, temp_dir) = create_test_service();

        // Create test configuration file
        let config_path = temp_dir.path().join("config.json");
        fs::write(&config_path, r#"{"test": "data"}"#).unwrap();

        // Create multiple backups
        let id1 = service.create_backup(&config_path).unwrap();
        let id2 = service.create_backup(&config_path).unwrap();

        // List backups
        let backups = service.list_backups().unwrap();
        assert_eq!(backups.len(), 2);

        // Verify newest first
        assert!(backups[0].created_at >= backups[1].created_at);
    }
}