use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};
use base64::{Engine, engine::general_purpose::STANDARD as Base64Engine};
use serde_json::{json, Value};
use tauri::command;

// 错误类型
#[derive(Debug)]
pub enum CloudSyncError {
    Io(String),
    Parse(String),
    Network(String),
    Encryption(String),
    GitHub(String),
}

impl std::fmt::Display for CloudSyncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(msg) => write!(f, "IO error: {}", msg),
            Self::Parse(msg) => write!(f, "Parse error: {}", msg),
            Self::Network(msg) => write!(f, "Network error: {}", msg),
            Self::Encryption(msg) => write!(f, "Encryption error: {}", msg),
            Self::GitHub(msg) => write!(f, "GitHub error: {}", msg),
        }
    }
}

impl std::error::Error for CloudSyncError {}

pub type CloudSyncResult<T> = Result<T, CloudSyncError>;

// 云同步设置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloudSyncSettings {
    pub enabled: bool,
    pub github_token: Option<String>,
    pub gist_url: Option<String>,
    pub last_sync_timestamp: Option<DateTime<Utc>>,
}

impl Default for CloudSyncSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            github_token: None,
            gist_url: None,
            last_sync_timestamp: None,
        }
    }
}

impl CloudSyncSettings {
    fn settings_path() -> PathBuf {
        dirs::home_dir()
            .expect("Cannot find home directory")
            .join(".cc-switch")
            .join("sync-settings.json")
    }

    pub fn load() -> CloudSyncResult<Option<Self>> {
        let path = Self::settings_path();
        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .map_err(|e| CloudSyncError::Io(format!("Failed to read settings: {}", e)))?;

        let settings = serde_json::from_str(&content)
            .map_err(|e| CloudSyncError::Parse(format!("Failed to parse settings: {}", e)))?;

        Ok(Some(settings))
    }

    pub fn save(&self) -> CloudSyncResult<()> {
        let path = Self::settings_path();

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| CloudSyncError::Io(format!("Failed to create directory: {}", e)))?;
        }

        let json = serde_json::to_string_pretty(self)
            .map_err(|e| CloudSyncError::Parse(format!("Failed to serialize: {}", e)))?;

        fs::write(&path, json)
            .map_err(|e| CloudSyncError::Io(format!("Failed to write settings: {}", e)))?;

        Ok(())
    }
}

// 使用 OnceLock 模式管理设置（参考 settings.rs）
fn cloud_sync_store() -> &'static RwLock<CloudSyncSettings> {
    static STORE: OnceLock<RwLock<CloudSyncSettings>> = OnceLock::new();
    STORE.get_or_init(|| {
        let settings = CloudSyncSettings::load()
            .ok()
            .flatten()
            .unwrap_or_default();
        RwLock::new(settings)
    })
}

// 简单的备份功能
fn create_backup(config_path: &PathBuf) -> Result<String, String> {
    let backup_dir = dirs::home_dir()
        .ok_or_else(|| "Cannot find home directory".to_string())?
        .join(".cc-switch")
        .join("backups");

    fs::create_dir_all(&backup_dir)
        .map_err(|e| format!("Failed to create backup directory: {}", e))?;

    let timestamp = Utc::now().format("%Y%m%d_%H%M%S");
    let backup_id = format!("backup_{}", timestamp);
    let backup_path = backup_dir.join(format!("{}.json", backup_id));

    fs::copy(config_path, &backup_path)
        .map_err(|e| format!("Failed to create backup: {}", e))?;

    Ok(backup_id)
}

// GitHub API 客户端
struct GitHubClient {
    token: String,
    client: reqwest::Client,
}

impl GitHubClient {
    fn new(token: String) -> Self {
        Self {
            token,
            client: reqwest::Client::new(),
        }
    }

    async fn validate_token(&self) -> Result<Value, String> {
        let response = self.client
            .get("https://api.github.com/user")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "cc-switch")
            .send()
            .await
            .map_err(|e| format!("Network error: {}", e))?;

        if response.status().is_success() {
            let user_info = response.json::<Value>().await
                .map_err(|e| format!("Failed to parse response: {}", e))?;
            Ok(user_info)
        } else {
            Err(format!("Invalid token: {}", response.status()))
        }
    }

    async fn create_gist(&self, content: &str) -> Result<String, String> {
        let gist_data = json!({
            "description": "CC-Switch Configuration Backup",
            "public": false,
            "files": {
                "cc-switch-config.json": {
                    "content": content
                }
            }
        });

        let response = self.client
            .post("https://api.github.com/gists")
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "cc-switch")
            .json(&gist_data)
            .send()
            .await
            .map_err(|e| format!("Failed to create gist: {}", e))?;

        if response.status().is_success() {
            let gist = response.json::<Value>().await
                .map_err(|e| format!("Failed to parse gist response: {}", e))?;

            gist["html_url"].as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Failed to get gist URL".to_string())
        } else {
            Err(format!("Failed to create gist: {}", response.status()))
        }
    }

    async fn update_gist_by_url(&self, gist_url: &str, content: &str) -> Result<(), String> {
        // Extract gist ID from URL
        let gist_id = gist_url
            .split('/')
            .last()
            .ok_or_else(|| "Invalid gist URL".to_string())?;

        let update_data = json!({
            "files": {
                "cc-switch-config.json": {
                    "content": content
                }
            }
        });

        let response = self.client
            .patch(&format!("https://api.github.com/gists/{}", gist_id))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "cc-switch")
            .json(&update_data)
            .send()
            .await
            .map_err(|e| format!("Failed to update gist: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(format!("Failed to update gist: {}", response.status()))
        }
    }

    async fn get_gist(&self, gist_url: &str) -> Result<String, String> {
        // Extract gist ID from URL
        let gist_id = gist_url
            .split('/')
            .last()
            .ok_or_else(|| "Invalid gist URL".to_string())?;

        let response = self.client
            .get(&format!("https://api.github.com/gists/{}", gist_id))
            .header("Authorization", format!("Bearer {}", self.token))
            .header("User-Agent", "cc-switch")
            .send()
            .await
            .map_err(|e| format!("Failed to fetch gist: {}", e))?;

        if response.status().is_success() {
            let gist = response.json::<Value>().await
                .map_err(|e| format!("Failed to parse gist: {}", e))?;

            gist["files"]["cc-switch-config.json"]["content"]
                .as_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "Failed to get gist content".to_string())
        } else {
            Err(format!("Failed to fetch gist: {}", response.status()))
        }
    }
}

// 加密服务
mod crypto {
    use chacha20poly1305::{
        aead::{Aead, AeadCore, KeyInit, OsRng},
        ChaCha20Poly1305, Nonce,
    };
    use pbkdf2::pbkdf2_hmac;
    use sha2::Sha256;

    const SALT_LEN: usize = 32;
    const KEY_LEN: usize = 32;
    const NONCE_LEN: usize = 12;
    const ITERATIONS: u32 = 100_000;

    pub fn encrypt(data: &[u8], password: &str) -> Result<Vec<u8>, String> {
        // Generate random salt
        let mut salt = [0u8; SALT_LEN];
        use rand::RngCore;
        OsRng.fill_bytes(&mut salt);

        // Derive key from password
        let mut key = [0u8; KEY_LEN];
        pbkdf2_hmac::<Sha256>(password.as_bytes(), &salt, ITERATIONS, &mut key);

        // Create cipher
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "Failed to create cipher")?;

        // Generate random nonce
        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng);

        // Encrypt data
        let encrypted = cipher
            .encrypt(&nonce, data)
            .map_err(|_| "Encryption failed")?;

        // Build output: salt + nonce + encrypted_data
        let mut result = Vec::new();
        result.extend_from_slice(&salt);
        result.extend_from_slice(&nonce);
        result.extend_from_slice(&encrypted);

        Ok(result)
    }

    pub fn decrypt(data: &[u8], password: &str) -> Result<Vec<u8>, String> {
        if data.len() < SALT_LEN + NONCE_LEN {
            return Err("Invalid encrypted data".to_string());
        }

        // Split data
        let (salt, rest) = data.split_at(SALT_LEN);
        let (nonce, encrypted) = rest.split_at(NONCE_LEN);

        // Derive key from password
        let mut key = [0u8; KEY_LEN];
        pbkdf2_hmac::<Sha256>(password.as_bytes(), salt, ITERATIONS, &mut key);

        // Create cipher
        let cipher = ChaCha20Poly1305::new_from_slice(&key)
            .map_err(|_| "Failed to create cipher")?;

        // Decrypt data
        let nonce = Nonce::from_slice(nonce);
        cipher
            .decrypt(nonce, encrypted)
            .map_err(|_| "Decryption failed - invalid password or corrupted data".to_string())
    }
}

// ============= Tauri Commands =============

#[command]
pub async fn configure_cloud_sync(
    github_token: String,
    gist_url: Option<String>,
) -> Result<Value, String> {
    // Validate token first if provided
    if !github_token.is_empty() {
        let client = GitHubClient::new(github_token.clone());
        client.validate_token().await?;
    }

    // Update settings after validation
    let gist_url = {
        let mut guard = cloud_sync_store()
            .write()
            .map_err(|_| "Failed to acquire lock")?;

        // Only update token if provided
        if !github_token.is_empty() {
            guard.github_token = Some(github_token);
        }

        // Update other settings
        if let Some(url) = gist_url {
            guard.gist_url = Some(url.clone());
        }
        guard.enabled = true;

        // Save settings
        guard.save()
            .map_err(|e| format!("Failed to save settings: {}", e))?;

        guard.gist_url.clone()
    };

    Ok(json!({
        "success": true,
        "message": "Cloud sync configured successfully",
        "gistUrl": gist_url
    }))
}

#[command]
pub async fn get_cloud_sync_settings() -> Result<Value, String> {
    let guard = cloud_sync_store()
        .read()
        .map_err(|_| "Failed to acquire lock")?;

    Ok(json!({
        "configured": guard.enabled,
        "gistUrl": guard.gist_url,
        "enabled": guard.enabled,
        "lastSyncTimestamp": guard.last_sync_timestamp,
        "hasToken": guard.github_token.is_some()
    }))
}

#[command]
pub async fn sync_to_cloud(encryption_password: String) -> Result<Value, String> {
    // Get settings
    let settings = {
        let guard = cloud_sync_store()
            .read()
            .map_err(|_| "Failed to acquire lock")?;
        guard.clone()
    };

    if !settings.enabled {
        return Err("Cloud sync not configured".to_string());
    }

    let github_token = settings.github_token
        .ok_or_else(|| "GitHub token not configured".to_string())?;

    // Get configuration file
    let config_path = crate::config::get_app_config_path();
    let config_content = fs::read_to_string(&config_path)
        .map_err(|e| format!("Failed to read configuration: {}", e))?;

    // Create backup
    let backup_id = create_backup(&config_path)?;

    // Encrypt configuration
    let encrypted_data = crypto::encrypt(config_content.as_bytes(), &encryption_password)
        .map_err(|e| format!("Failed to encrypt: {}", e))?;

    // Convert to base64
    let encoded_data = Base64Engine.encode(&encrypted_data);

    // Upload to GitHub
    let client = GitHubClient::new(github_token);
    let gist_url = if let Some(existing_url) = &settings.gist_url {
        // Update existing gist
        match client.update_gist_by_url(existing_url, &encoded_data).await {
            Ok(_) => existing_url.clone(),
            Err(_) => {
                // If update fails, create new one
                client.create_gist(&encoded_data).await?
            }
        }
    } else {
        // Create new gist
        client.create_gist(&encoded_data).await?
    };

    // Update settings
    {
        let mut guard = cloud_sync_store()
            .write()
            .map_err(|_| "Failed to acquire lock")?;
        guard.gist_url = Some(gist_url.clone());
        guard.last_sync_timestamp = Some(Utc::now());
        guard.save()
            .map_err(|e| format!("Failed to save settings: {}", e))?;
    }

    Ok(json!({
        "success": true,
        "gist_url": gist_url,
        "backup_id": backup_id,
        "message": "Configuration synced to cloud successfully"
    }))
}

#[command]
pub async fn sync_from_cloud(
    gist_url: String,
    encryption_password: String,
    auto_apply: bool,
) -> Result<Value, String> {
    // Get GitHub token
    let github_token = {
        let guard = cloud_sync_store()
            .read()
            .map_err(|_| "Failed to acquire lock")?;
        guard.github_token.clone()
            .ok_or_else(|| "GitHub token not configured".to_string())?
    };

    // Download from GitHub
    let client = GitHubClient::new(github_token);
    let encoded_data = client.get_gist(&gist_url).await?;

    // Decode and decrypt
    let encrypted_data = Base64Engine.decode(&encoded_data)
        .map_err(|e| format!("Failed to decode: {}", e))?;

    let config_content = crypto::decrypt(&encrypted_data, &encryption_password)?;
    let config_content = String::from_utf8(config_content)
        .map_err(|e| format!("Invalid UTF-8: {}", e))?;

    if auto_apply {
        // Get config path
        let config_path = crate::config::get_app_config_path();

        // Create backup
        let backup_id = create_backup(&config_path)?;

        // Write new configuration
        fs::write(&config_path, &config_content)
            .map_err(|e| format!("Failed to write configuration: {}", e))?;

        // Update timestamp
        {
            let mut guard = cloud_sync_store()
                .write()
                .map_err(|_| "Failed to acquire lock")?;
            guard.last_sync_timestamp = Some(Utc::now());
            guard.save()
                .map_err(|e| format!("Failed to save settings: {}", e))?;
        }

        Ok(json!({
            "success": true,
            "applied": true,
            "backup_id": backup_id,
            "message": "Configuration synced from cloud and applied"
        }))
    } else {
        Ok(json!({
            "success": true,
            "applied": false,
            "configuration": config_content,
            "message": "Configuration downloaded from cloud"
        }))
    }
}

#[command]
pub async fn validate_github_token(github_token: String) -> Result<Value, String> {
    let client = GitHubClient::new(github_token);

    match client.validate_token().await {
        Ok(user_info) => Ok(json!({
            "valid": true,
            "user": user_info,
            "message": "Token is valid"
        })),
        Err(e) => Ok(json!({
            "valid": false,
            "message": format!("Token validation failed: {}", e)
        }))
    }
}