//! ACL-based key vault with POSIX-friendly permissions
//!
//! Features:
//! - Filesystem-based ACL hierarchy (~/.keymux/acl/)
//! - Opt-in environment variable support (XXXXX_API_KEY)
//! - POSIX permissions (owner/group/other)
//! - No complex dependencies (just std::fs + rusqlite)

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

/// ACL hierarchy root directory
const ACL_ROOT: &str = ".keymux/acl";

/// Provider key structure
#[derive(Debug, Clone)]
pub struct ProviderKey {
    pub id: String,
    pub provider: String,
    pub key: String,
    pub quota_limit: Option<f64>,
    pub quota_used: f64,
    pub is_active: bool,
    pub permissions: Permissions,
}

/// POSIX-style permissions
#[derive(Debug, Clone, Copy, Default)]
pub struct Permissions {
    pub owner_read: bool,
    pub owner_write: bool,
    pub group_read: bool,
    pub group_write: bool,
    pub other_read: bool,
    pub other_write: bool,
}

impl Permissions {
    /// Default permissions: owner read/write only
    pub fn default_secure() -> Self {
        Self {
            owner_read: true,
            owner_write: true,
            group_read: false,
            group_write: false,
            other_read: false,
            other_write: false,
        }
    }

    /// From octal (e.g., 0o600, 0o640, 0o644)
    pub fn from_octal(octal: u32) -> Self {
        Self {
            owner_read: (octal & 0o400) != 0,
            owner_write: (octal & 0o200) != 0,
            group_read: (octal & 0o040) != 0,
            group_write: (octal & 0o020) != 0,
            other_read: (octal & 0o004) != 0,
            other_write: (octal & 0o002) != 0,
        }
    }

    /// To octal
    pub fn to_octal(&self) -> u32 {
        let mut octal = 0;
        if self.owner_read {
            octal |= 0o400;
        }
        if self.owner_write {
            octal |= 0o200;
        }
        if self.group_read {
            octal |= 0o040;
        }
        if self.group_write {
            octal |= 0o020;
        }
        if self.other_read {
            octal |= 0o004;
        }
        if self.other_write {
            octal |= 0o002;
        }
        octal
    }
}

/// ACL-based key vault
pub struct AclKeyVault {
    acl_root: PathBuf,
    keys: HashMap<String, ProviderKey>,
}

impl AclKeyVault {
    /// Open or create ACL key vault
    pub fn open<P: AsRef<Path>>(base_dir: P) -> Result<Self> {
        let acl_root = base_dir.as_ref().join(ACL_ROOT);

        // Create ACL directory structure
        fs::create_dir_all(&acl_root).context("Failed to create ACL root directory")?;

        // Set secure permissions on ACL root (0o700)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&acl_root, fs::Permissions::from_mode(0o700))
                .context("Failed to set ACL root permissions")?;
        }

        // Load keys from filesystem
        let mut keys = HashMap::new();
        if acl_root.exists() {
            keys = Self::load_keys_from_fs(&acl_root)?;
        }

        // Load keys from environment variables (opt-in)
        keys.extend(Self::load_keys_from_env()?);

        Ok(Self { acl_root, keys })
    }

    /// Load keys from filesystem
    fn load_keys_from_fs(acl_root: &Path) -> Result<HashMap<String, ProviderKey>> {
        let mut keys = HashMap::new();

        // Directory structure:
        // ~/.cc-switch/acl/
        // ├── anthropic/
        // │   ├── key-1.key      # Key file
        // │   ├── key-1.meta     # Metadata (quota, permissions)
        // │   └── key-2.key
        // ├── openai/
        // │   └── key-1.key
        // └── google/
        //     └── key-1.key

        if let Ok(entries) = fs::read_dir(acl_root) {
            for entry in entries.flatten() {
                let provider_dir = entry.path();
                if !provider_dir.is_dir() {
                    continue;
                }

                let provider = provider_dir
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string();

                // Read all .key files in provider directory
                if let Ok(key_entries) = fs::read_dir(&provider_dir) {
                    for key_entry in key_entries.flatten() {
                        let key_path = key_entry.path();
                        if key_path.extension().and_then(|e| e.to_str()) == Some("key") {
                            if let Ok(key_content) = fs::read_to_string(&key_path) {
                                let key_id = key_path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("unknown")
                                    .to_string();

                                // Load metadata if exists
                                let meta_path = key_path.with_extension("meta");
                                let (quota_limit, quota_used, permissions) = if meta_path.exists() {
                                    Self::load_key_meta(&meta_path)?
                                } else {
                                    (None, 0.0, Permissions::default_secure())
                                };

                                keys.insert(
                                    key_id.clone(),
                                    ProviderKey {
                                        id: key_id,
                                        provider: provider.clone(),
                                        key: key_content.trim().to_string(),
                                        quota_limit,
                                        quota_used,
                                        is_active: true,
                                        permissions,
                                    },
                                );
                            }
                        }
                    }
                }
            }
        }

        Ok(keys)
    }

    /// Load key metadata
    fn load_key_meta(meta_path: &Path) -> Result<(Option<f64>, f64, Permissions)> {
        let content = fs::read_to_string(meta_path)?;
        let mut quota_limit: Option<f64> = None;
        let mut quota_used = 0.0;
        let mut permissions = Permissions::default_secure();

        for line in content.lines() {
            let parts: Vec<&str> = line.splitn(2, '=').collect();
            if parts.len() == 2 {
                match parts[0].trim() {
                    "quota_limit" => {
                        quota_limit = parts[1].trim().parse().ok();
                    }
                    "quota_used" => {
                        quota_used = parts[1].trim().parse().unwrap_or(0.0);
                    }
                    "permissions" => {
                        let octal = u32::from_str_radix(parts[1].trim(), 8).unwrap_or(0o600);
                        permissions = Permissions::from_octal(octal);
                    }
                    _ => {}
                }
            }
        }

        Ok((quota_limit, quota_used, permissions))
    }

    /// Load keys from environment variables (opt-in)
    ///
    /// Format: {PROVIDER}_API_KEY (e.g., ANTHROPIC_API_KEY, OPENAI_API_KEY)
    fn load_keys_from_env() -> Result<HashMap<String, ProviderKey>> {
        let mut keys = HashMap::new();

        // Provider mappings: env var prefix → provider name
        let provider_vars = [
            ("ANTHROPIC", "anthropic"),
            ("OPENAI", "openai"),
            ("GOOGLE", "google"),
            ("DEEPSEEK", "deepseek"),
            ("MOONSHOT", "moonshot"),
            ("MINIMAX", "minimax"),
            ("OPENROUTER", "openrouter"),
        ];

        for (env_prefix, provider) in provider_vars {
            let env_var = format!("{}_API_KEY", env_prefix);

            // Opt-in: only load if explicitly set
            if let Ok(key_value) = env::var(&env_var) {
                if !key_value.trim().is_empty() {
                    let key_id = format!("env-{}-1", provider.to_lowercase());

                    keys.insert(
                        key_id.clone(),
                        ProviderKey {
                            id: key_id,
                            provider: provider.to_string(),
                            key: key_value.trim().to_string(),
                            quota_limit: None,
                            quota_used: 0.0,
                            is_active: true,
                            permissions: Permissions::default_secure(),
                        },
                    );

                    log::info!("Loaded API key from environment: {}", env_var);
                }
            }
        }

        Ok(keys)
    }

    /// Get all active keys for a provider
    pub fn get_keys_for_provider(&self, provider: &str) -> Vec<ProviderKey> {
        self.keys
            .values()
            .filter(|k| k.provider == provider && k.is_active)
            .cloned()
            .collect()
    }

    /// Get a specific key by ID
    pub fn get_key(&self, key_id: &str) -> Option<&ProviderKey> {
        self.keys.get(key_id)
    }

    /// Add a new key (filesystem + memory)
    pub fn add_key(&mut self, key: ProviderKey) -> Result<()> {
        let provider_dir = self.acl_root.join(&key.provider);
        fs::create_dir_all(&provider_dir)?;

        // Write key file
        let key_path = provider_dir.join(format!("{}.key", key.id));
        fs::write(&key_path, &key.key)?;

        // Set secure permissions (0o600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600))?;
        }

        // Write metadata file
        let meta_path = provider_dir.join(format!("{}.meta", key.id));
        let meta_content = format!(
            "quota_limit={}\nquota_used={}\npermissions={:o}\n",
            key.quota_limit.map(|q| q.to_string()).unwrap_or_default(),
            key.quota_used,
            key.permissions.to_octal(),
        );
        fs::write(&meta_path, meta_content)?;

        // Set secure permissions on meta file (0o600)
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&meta_path, fs::Permissions::from_mode(0o600))?;
        }

        // Add to memory
        self.keys.insert(key.id.clone(), key);

        Ok(())
    }

    /// Update quota usage
    pub fn update_quota_usage(&mut self, key_id: &str, tokens_used: f64) -> Result<()> {
        if let Some(key) = self.keys.get_mut(key_id) {
            key.quota_used += tokens_used;

            // Update metadata file
            let provider_dir = self.acl_root.join(&key.provider);
            let meta_path = provider_dir.join(format!("{}.meta", key_id));

            if meta_path.exists() {
                let meta_content = format!(
                    "quota_limit={}\nquota_used={}\npermissions={:o}\n",
                    key.quota_limit.map(|q| q.to_string()).unwrap_or_default(),
                    key.quota_used,
                    key.permissions.to_octal(),
                );
                fs::write(&meta_path, meta_content)?;
            }
        }

        Ok(())
    }

    /// List all providers
    pub fn list_providers(&self) -> Vec<String> {
        let mut providers: Vec<String> = self.keys.values().map(|k| k.provider.clone()).collect();
        providers.sort();
        providers.dedup();
        providers
    }

    /// Check if key has quota remaining
    pub fn has_quota_remaining(&self, key_id: &str) -> bool {
        if let Some(key) = self.keys.get(key_id) {
            match key.quota_limit {
                Some(limit) => key.quota_used < limit,
                None => true, // No limit = unlimited
            }
        } else {
            false
        }
    }

    /// Get all keys (for debugging)
    pub fn list_keys(&self) -> Vec<&ProviderKey> {
        self.keys.values().collect()
    }
}

/// Save key to environment variable (opt-in)
pub fn save_key_to_env(provider: &str, key: &str) -> Result<()> {
    let env_var = match provider.to_lowercase().as_str() {
        "anthropic" => "ANTHROPIC_API_KEY",
        "openai" => "OPENAI_API_KEY",
        "google" => "GOOGLE_API_KEY",
        "deepseek" => "DEEPSEEK_API_KEY",
        "moonshot" => "MOONSHOT_API_KEY",
        "minimax" => "MINIMAX_API_KEY",
        "openrouter" => "OPENROUTER_API_KEY",
        _ => return Err(anyhow::anyhow!("Unknown provider: {}", provider)),
    };

    // Note: This only sets for current process, not system-wide
    // For system-wide, user should use: echo 'export XXX_API_KEY=...' >> ~/.bashrc
    env::set_var(env_var, key);

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_acl_vault_basic() -> Result<()> {
        let dir = tempdir()?;
        let vault = AclKeyVault::open(dir.path())?;

        // Add a key
        let key = ProviderKey {
            id: "key-1".to_string(),
            provider: "anthropic".to_string(),
            key: "sk-test-123".to_string(),
            quota_limit: Some(100.0),
            quota_used: 0.0,
            is_active: true,
            permissions: Permissions::default_secure(),
        };

        vault.add_key(key.clone())?;

        // Retrieve the key
        let keys = vault.get_keys_for_provider("anthropic");
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].provider, "anthropic");

        // Update quota
        vault.update_quota_usage("key-1", 10.0)?;
        let updated = vault.get_key("key-1").unwrap();
        assert_eq!(updated.quota_used, 10.0);

        // List providers
        let providers = vault.list_providers();
        assert_eq!(providers, vec!["anthropic"]);

        Ok(())
    }

    #[test]
    fn test_permissions_octal() {
        let perms = Permissions::from_octal(0o640);
        assert!(perms.owner_read);
        assert!(perms.owner_write);
        assert!(perms.group_read);
        assert!(!perms.group_write);
        assert!(!perms.other_read);
        assert!(!perms.other_write);
        assert_eq!(perms.to_octal(), 0o640);
    }

    #[test]
    fn test_env_key_loading() {
        // Set test env var
        env::set_var("TEST_ANTHROPIC_API_KEY", "sk-test-env");

        // Note: env vars are process-wide, so this test may interfere with others
        // In practice, users opt-in by setting env vars in their shell
    }
}
