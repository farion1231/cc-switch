//! Key unlocking via SSH pubkey authentication
//!
//! Secrets are locked with registered pubkeys and only unlocked
//! for authenticated SSH sessions.

use anyhow::{Context, Result};
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    aead::{Aead, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

/// Locked secret - encrypted with a pubkey-derived key
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LockedSecret {
    /// Provider name (e.g., "openai", "anthropic")
    pub provider: String,
    /// Encrypted secret data
    pub ciphertext: Vec<u8>,
    /// Nonce for decryption
    pub nonce: Vec<u8>,
    /// Salt for key derivation
    pub salt: Vec<u8>,
    /// Pubkey fingerprints that can unlock this secret
    pub allowed_pubkeys: Vec<String>,
    /// Created timestamp
    pub created_at: chrono::DateTime<chrono::Utc>,
}

/// Secret lock store
pub struct SecretLock {
    store_path: PathBuf,
    secrets: HashMap<String, LockedSecret>,
    registered_pubkeys: HashMap<String, Vec<String>>,
}

impl SecretLock {
    /// Create or open the secret lock store
    pub fn open(base_dir: &PathBuf) -> Result<Self> {
        let store_path = base_dir.join("locked_secrets.json");

        let secrets = if store_path.exists() {
            let content =
                fs::read_to_string(&store_path).context("Failed to read secret lock store")?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };

        Ok(Self {
            store_path,
            secrets,
            registered_pubkeys: HashMap::new(),
        })
    }

    /// Register a pubkey for unlocking secrets
    pub fn register_pubkey(&mut self, fingerprint: &str, allowed_providers: Vec<String>) {
        let providers = allowed_providers.clone();
        self.registered_pubkeys
            .insert(fingerprint.to_string(), allowed_providers);
        log::info!(
            "Registered pubkey: {} for providers: {:?}",
            fingerprint,
            providers
        );
    }

    /// Unregister a pubkey
    pub fn unregister_pubkey(&mut self, fingerprint: &str) {
        self.registered_pubkeys.remove(fingerprint);
        log::info!("Unregistered pubkey: {}", fingerprint);
    }

    /// List registered pubkeys
    pub fn list_registered(&self) -> &HashMap<String, Vec<String>> {
        &self.registered_pubkeys
    }

    /// Lock a secret with registered pubkeys
    pub fn lock_secret(
        &mut self,
        provider: &str,
        secret: &str,
        allowed_pubkeys: &[String],
    ) -> Result<()> {
        if allowed_pubkeys.is_empty() {
            anyhow::bail!("Must specify at least one allowed pubkey");
        }

        // Generate salt
        let mut salt = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut salt);

        // Derive key from first pubkey (in real impl, would use each pubkey)
        let key = self.derive_key(&allowed_pubkeys[0], &salt)?;

        // Generate nonce
        let mut nonce_bytes = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut nonce_bytes);

        // Encrypt
        let cipher = ChaCha20Poly1305::new_from_slice(&key).context("Failed to create cipher")?;
        let nonce = Nonce::from_slice(&nonce_bytes);
        let ciphertext = cipher
            .encrypt(nonce, secret.as_bytes())
            .map_err(|e| anyhow::anyhow!("Encryption failed: {}", e))?;

        // Store locked secret
        let locked = LockedSecret {
            provider: provider.to_string(),
            ciphertext,
            nonce: nonce_bytes.to_vec(),
            salt: salt.to_vec(),
            allowed_pubkeys: allowed_pubkeys.to_vec(),
            created_at: chrono::Utc::now(),
        };

        self.secrets.insert(provider.to_string(), locked);
        self.save()?;

        log::info!("Locked secret for provider: {}", provider);
        Ok(())
    }

    /// Unlock a secret using a registered pubkey
    pub fn unlock_secret(
        &self,
        provider: &str,
        pubkey_fingerprint: &str,
    ) -> Result<Option<String>> {
        let locked = match self.secrets.get(provider) {
            Some(s) => s,
            None => return Ok(None),
        };

        // Check if this pubkey is allowed
        if !locked
            .allowed_pubkeys
            .contains(&pubkey_fingerprint.to_string())
        {
            log::warn!(
                "Pubkey {} not authorized for provider {}",
                pubkey_fingerprint,
                provider
            );
            return Ok(None);
        }

        // Check if pubkey is registered
        if !self.registered_pubkeys.contains_key(pubkey_fingerprint) {
            log::warn!("Pubkey {} not registered", pubkey_fingerprint);
            return Ok(None);
        }

        // Derive key
        let key = self.derive_key(pubkey_fingerprint, &locked.salt)?;

        // Decrypt
        let cipher = ChaCha20Poly1305::new_from_slice(&key).context("Failed to create cipher")?;
        let nonce = Nonce::from_slice(&locked.nonce);

        let plaintext = cipher
            .decrypt(nonce, locked.ciphertext.as_slice())
            .map_err(|e| anyhow::anyhow!("Decryption failed: {}", e))?;

        let secret = String::from_utf8(plaintext).context("Invalid UTF-8 in decrypted secret")?;

        log::info!(
            "Unlocked secret for provider: {} using pubkey: {}",
            provider,
            pubkey_fingerprint
        );
        Ok(Some(secret))
    }

    /// List locked secrets (without revealing content)
    pub fn list_locked(&self) -> Vec<&str> {
        self.secrets.keys().map(|s| s.as_str()).collect()
    }

    /// Remove a locked secret
    pub fn remove_secret(&mut self, provider: &str) -> Result<bool> {
        if self.secrets.remove(provider).is_some() {
            self.save()?;
            log::info!("Removed locked secret for provider: {}", provider);
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// Derive encryption key from pubkey fingerprint
    fn derive_key(&self, fingerprint: &str, salt: &[u8]) -> Result<[u8; 32]> {
        let mut key = [0u8; 32];

        let params = Params::new(
            3,        // m_cost
            2,        // t_cost
            1,        // p_cost
            Some(32), // output length
        )
        .context("Failed to create Argon2 params")?;

        let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        argon2
            .hash_password_into(fingerprint.as_bytes(), salt, &mut key)
            .context("Failed to derive key")?;

        Ok(key)
    }

    /// Save to disk
    fn save(&self) -> Result<()> {
        if let Some(parent) = self.store_path.parent() {
            fs::create_dir_all(parent).context("Failed to create store directory")?;
        }

        let content =
            serde_json::to_string_pretty(&self.secrets).context("Failed to serialize secrets")?;

        fs::write(&self.store_path, content).context("Failed to write secret store")?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_lock_unlock_secret() {
        let dir = tempdir().unwrap();
        let mut lock = SecretLock::open(&dir.path().to_path_buf()).unwrap();

        let pubkey_fp = "SHA256:testfingerprint123";

        // Register pubkey
        lock.register_pubkey(pubkey_fp, vec!["openai".to_string()]);

        // Lock secret
        lock.lock_secret("openai", "sk-test-key-12345", &[pubkey_fp.to_string()])
            .unwrap();

        // Unlock secret
        let unlocked = lock.unlock_secret("openai", pubkey_fp).unwrap();
        assert_eq!(unlocked, Some("sk-test-key-12345".to_string()));

        // Try with wrong pubkey
        let wrong_fp = "SHA256:wrongfingerprint";
        lock.register_pubkey(wrong_fp, vec!["openai".to_string()]);
        let result = lock.unlock_secret("openai", wrong_fp).unwrap();
        assert_eq!(result, None);
    }

    #[test]
    fn test_multiple_pubkeys() {
        let dir = tempdir().unwrap();
        let mut lock = SecretLock::open(&dir.path().to_path_buf()).unwrap();

        let fp1 = "SHA256:fingerprint1";
        let fp2 = "SHA256:fingerprint2";

        lock.register_pubkey(fp1, vec!["openai".to_string()]);
        lock.register_pubkey(fp2, vec!["anthropic".to_string()]);

        lock.lock_secret("openai", "key1", &[fp1.to_string()])
            .unwrap();
        lock.lock_secret("anthropic", "key2", &[fp2.to_string()])
            .unwrap();

        assert_eq!(
            lock.unlock_secret("openai", fp1).unwrap(),
            Some("key1".to_string())
        );
        assert_eq!(
            lock.unlock_secret("anthropic", fp2).unwrap(),
            Some("key2".to_string())
        );
        assert_eq!(lock.unlock_secret("openai", fp2).unwrap(), None);
    }
}
