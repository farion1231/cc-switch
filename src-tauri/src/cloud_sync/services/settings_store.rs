use crate::cloud_sync::{CloudSyncResult, CloudSyncError, models::CloudSyncSettings};
use std::path::PathBuf;
use std::fs;
use std::io::Write;
use serde_json;

pub struct SettingsStore {
    settings_path: PathBuf,
}

impl SettingsStore {
    pub fn new() -> Self {
        let settings_path = Self::default_settings_path();
        Self { settings_path }
    }

    #[allow(dead_code)]
    pub fn with_path(settings_path: PathBuf) -> Self {
        Self { settings_path }
    }

    pub fn load(&self) -> CloudSyncResult<Option<CloudSyncSettings>> {
        if !self.settings_path.exists() {
            return Ok(None);
        }

        let settings_json = fs::read_to_string(&self.settings_path)
            .map_err(|e| CloudSyncError::Io(format!("Failed to read settings file: {}", e)))?;

        let settings: CloudSyncSettings = serde_json::from_str(&settings_json)
            .map_err(|e| CloudSyncError::Parse(format!("Failed to parse settings: {}", e)))?;

        Ok(Some(settings))
    }

    pub fn save(&self, settings: &CloudSyncSettings) -> CloudSyncResult<()> {
        // Ensure parent directory exists
        if let Some(parent) = self.settings_path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CloudSyncError::Io(format!("Failed to create settings directory: {}", e)))?;
        }

        let settings_json = serde_json::to_string_pretty(settings)
            .map_err(|e| CloudSyncError::Parse(format!("Failed to serialize settings: {}", e)))?;

        let mut file = fs::File::create(&self.settings_path)
            .map_err(|e| CloudSyncError::Io(format!("Failed to create settings file: {}", e)))?;

        file.write_all(settings_json.as_bytes())
            .map_err(|e| CloudSyncError::Io(format!("Failed to write settings file: {}", e)))?;

        Ok(())
    }

    #[allow(dead_code)]
    pub fn delete(&self) -> CloudSyncResult<()> {
        if self.settings_path.exists() {
            fs::remove_file(&self.settings_path)
                .map_err(|e| CloudSyncError::Io(format!("Failed to delete settings file: {}", e)))?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub fn exists(&self) -> bool {
        self.settings_path.exists()
    }

    fn default_settings_path() -> PathBuf {
        dirs::home_dir()
            .expect("Cannot find home directory")
            .join(".cc-switch")
            .join("sync-settings.json")
    }

    #[allow(dead_code)]
    pub fn update<F>(&self, update_fn: F) -> CloudSyncResult<()>
    where
        F: FnOnce(&mut CloudSyncSettings) -> CloudSyncResult<()>,
    {
        let mut settings = self.load()?.unwrap_or_else(CloudSyncSettings::default);
        update_fn(&mut settings)?;
        self.save(&settings)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use crate::cloud_sync::models::SyncMode;

    fn create_test_store() -> (SettingsStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let settings_path = temp_dir.path().join("test-settings.json");
        let store = SettingsStore::with_path(settings_path);
        (store, temp_dir)
    }

    #[test]
    fn test_save_and_load_settings() {
        let (store, _temp_dir) = create_test_store();

        // Create test settings
        let mut settings = CloudSyncSettings::default();
        settings.enabled = true;
        settings.gist_url = Some("https://gist.github.com/user/abc123".to_string());
        settings.github_token = Some("test_token".to_string());
        settings.sync_mode = SyncMode::Automatic;

        // Save settings
        store.save(&settings).unwrap();

        // Load settings
        let loaded = store.load().unwrap().unwrap();
        assert_eq!(loaded.enabled, settings.enabled);
        assert_eq!(loaded.gist_url, settings.gist_url);
        assert_eq!(loaded.github_token, settings.github_token);
        assert_eq!(loaded.sync_mode, settings.sync_mode);
    }

    #[test]
    fn test_load_non_existent_returns_none() {
        let (store, _temp_dir) = create_test_store();
        let result = store.load().unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_update_settings() {
        let (store, _temp_dir) = create_test_store();

        // Initial save
        let mut settings = CloudSyncSettings::default();
        settings.enabled = false;
        store.save(&settings).unwrap();

        // Update
        store.update(|s| {
            s.enabled = true;
            s.sync_interval_minutes = 30;
            Ok(())
        }).unwrap();

        // Verify update
        let loaded = store.load().unwrap().unwrap();
        assert!(loaded.enabled);
        assert_eq!(loaded.sync_interval_minutes, 30);
    }

    #[test]
    fn test_delete_settings() {
        let (store, _temp_dir) = create_test_store();

        // Save settings
        let settings = CloudSyncSettings::default();
        store.save(&settings).unwrap();
        assert!(store.exists());

        // Delete
        store.delete().unwrap();
        assert!(!store.exists());

        // Load should return None
        assert!(store.load().unwrap().is_none());
    }
}