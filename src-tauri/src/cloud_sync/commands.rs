use serde_json::{json, Value};
use tauri::command;
use chrono::Utc;
use base64::{Engine, engine::general_purpose::STANDARD as Base64Engine};
use crate::cloud_sync::services::{
    crypto::CryptoService,
    github_client::GitHubClient,
    backup::BackupService,
    settings_store::SettingsStore,
};
use crate::cloud_sync::models::CloudSyncSettings;

#[command]
pub async fn configure_cloud_sync(
    github_token: String,
    gist_url: Option<String>,
    _encryption_password: String,
    _auto_sync_enabled: bool,
    _sync_on_startup: bool,
) -> Result<Value, String> {
    let settings_store = SettingsStore::new();

    // Load existing settings to preserve data
    let mut settings = match settings_store.load() {
        Ok(Some(existing)) => existing,
        Ok(None) => CloudSyncSettings::default(),
        Err(e) => return Err(format!("Failed to load existing settings: {}", e)),
    };

    // Only update token if a new one is provided
    if !github_token.is_empty() {
        // Validate GitHub token first
        let client = GitHubClient::new(github_token.clone());
        match client.validate_token().await {
            Ok(_) => {
                settings.github_token = Some(github_token);
            },
            Err(e) => return Err(format!("Token validation failed: {}", e)),
        }
    }

    // Update other settings
    if let Some(url) = gist_url {
        settings.gist_url = Some(url);
    }
    settings.enabled = true;

    // Save settings
    match settings_store.save(&settings) {
        Ok(_) => Ok(json!({
            "success": true,
            "message": "Cloud sync configured successfully"
        })),
        Err(e) => Err(format!("Failed to save settings: {}", e))
    }
}

#[command]
pub async fn get_cloud_sync_settings(_encryption_password: String) -> Result<Value, String> {
    let settings_store = SettingsStore::new();

    match settings_store.load() {
        Ok(Some(settings)) => Ok(json!({
            "configured": settings.enabled,
            "gistUrl": settings.gist_url,
            "enabled": settings.enabled,
            "syncMode": settings.sync_mode,
            "lastSyncTimestamp": settings.last_sync_timestamp,
            "hasToken": settings.github_token.is_some()
        })),
        Ok(None) => Ok(json!({
            "configured": false,
            "gistUrl": null,
            "enabled": false,
            "syncMode": "Manual",
            "lastSyncTimestamp": null,
            "hasToken": false
        })),
        Err(e) => Err(format!("Failed to load settings: {}", e))
    }
}

#[command]
pub async fn sync_to_cloud(
    encryption_password: String,
    _force_overwrite: bool,
) -> Result<Value, String> {
    // Load settings
    let settings_store = SettingsStore::new();
    let settings = match settings_store.load() {
        Ok(Some(settings)) => settings,
        Ok(None) => return Err("Cloud sync not configured".to_string()),
        Err(e) => return Err(format!("Failed to load settings: {}", e)),
    };

    // Get configuration file path
    let config_path = crate::config::get_app_config_path();

    // Read configuration
    let config_content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read configuration: {}", e))?;

    // Create backup
    let backup_service = BackupService::new();
    let backup_id = backup_service.create_backup(&config_path)
        .map_err(|e| format!("Failed to create backup: {}", e))?;

    // Encrypt configuration
    let crypto_service = CryptoService::new();
    let encrypted_data = crypto_service.encrypt(config_content.as_bytes(), &encryption_password)
        .map_err(|e| format!("Failed to encrypt configuration: {}", e))?;

    // Check if GitHub token is available
    let github_token = settings.github_token.as_ref()
        .ok_or_else(|| "GitHub token not configured".to_string())?;

    // Convert encrypted data to base64 for transmission
    let encoded_data = Base64Engine.encode(&encrypted_data);

    // Upload to GitHub
    let client = GitHubClient::new(github_token.clone());

    // Check if we already have a gist URL - if so, update it instead of creating a new one
    let gist_url = if let Some(existing_url) = &settings.gist_url {
        // Extract gist ID from existing URL and update it
        match client.update_gist_by_url(existing_url, &encoded_data).await {
            Ok(_) => {
                existing_url.clone()
            },
            Err(_e) => {
                // If update fails, try to create a new one
                match client.create_gist(&encoded_data).await {
                    Ok(url) => url,
                    Err(e) => return Err(format!("Failed to upload to cloud: {}", e)),
                }
            }
        }
    } else {
        // No existing gist, create a new one
        match client.create_gist(&encoded_data).await {
            Ok(url) => url,
            Err(e) => return Err(format!("Failed to upload to cloud: {}", e)),
        }
    };

    // Update settings with gist URL and timestamp
    let mut updated_settings = settings;
    updated_settings.gist_url = Some(gist_url.clone());
    updated_settings.last_sync_timestamp = Some(Utc::now());

    // Save updated settings
    match settings_store.save(&updated_settings) {
        Ok(_) => Ok(json!({
            "success": true,
            "gist_url": gist_url,
            "backup_id": backup_id,
            "message": "Configuration synced to cloud successfully"
        })),
        Err(e) => Err(format!("Sync succeeded but failed to update settings: {}", e))
    }
}

#[command]
pub async fn sync_from_cloud(
    gist_url: String,
    encryption_password: String,
    auto_apply: bool,
) -> Result<Value, String> {
    // Load settings to get GitHub token
    let settings_store = SettingsStore::new();
    let settings = match settings_store.load() {
        Ok(Some(settings)) => settings,
        Ok(None) => return Err("Cloud sync not configured".to_string()),
        Err(e) => return Err(format!("Failed to load settings: {}", e)),
    };

    // Check if GitHub token is available
    let github_token = settings.github_token.as_ref()
        .ok_or_else(|| "GitHub token not configured".to_string())?;

    // Download configuration from GitHub
    let client = GitHubClient::new(github_token.clone());
    let encoded_data = match client.get_gist(&gist_url).await {
        Ok(data) => data,
        Err(e) => return Err(format!("Failed to download from cloud: {}", e)),
    };

    // Decode from base64 and decrypt configuration
    let encrypted_data = Base64Engine.decode(&encoded_data)
        .map_err(|e| format!("Failed to decode base64 data: {}", e))?;

    let crypto_service = CryptoService::new();
    let config_content = crypto_service.decrypt(&encrypted_data, &encryption_password)
        .map_err(|e| format!("Failed to decrypt configuration: {}", e))?;

    let config_content = String::from_utf8(config_content)
        .map_err(|e| format!("Invalid UTF-8 in decrypted data: {}", e))?;

    if auto_apply {
        // Get current configuration path
        let config_path = crate::config::get_app_config_path();

        // Create backup before applying
        let backup_service = BackupService::new();
        let backup_id = backup_service.create_backup(&config_path)
            .map_err(|e| format!("Failed to create backup: {}", e))?;

        // Write new configuration
        std::fs::write(&config_path, &config_content)
            .map_err(|e| format!("Failed to write configuration: {}", e))?;

        // Update settings with timestamp
        let mut updated_settings = settings;
        updated_settings.last_sync_timestamp = Some(Utc::now());

        match settings_store.save(&updated_settings) {
            Ok(_) => Ok(json!({
                "success": true,
                "applied": true,
                "backup_id": backup_id,
                "message": "Configuration synced from cloud and applied successfully"
            })),
            Err(e) => Err(format!("Sync succeeded but failed to update settings: {}", e))
        }
    } else {
        // Return the configuration for manual review
        Ok(json!({
            "success": true,
            "applied": false,
            "configuration": config_content,
            "message": "Configuration downloaded from cloud. Review and apply manually."
        }))
    }
}

#[command]
pub async fn validate_github_token(github_token: String) -> Result<Value, String> {
    let client = GitHubClient::new(github_token);

    match client.validate_token().await {
        Ok(user_info) => {
            Ok(json!({
                "valid": true,
                "user": user_info,
                "message": "Token is valid"
            }))
        },
        Err(e) => {
            Ok(json!({
                "valid": false,
                "message": format!("Token validation failed: {}", e)
            }))
        }
    }
}