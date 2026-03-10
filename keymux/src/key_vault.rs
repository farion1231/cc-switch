//! Encrypted SQLite key vault (shared with keymux)

use crate::types::ApiKey;
use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::path::Path;
use std::sync::Arc;

/// Key encryption key (derived from master password)
/// In production, this should be derived from user's master password via Argon2
const KEY_ENCRYPTION_KEY: &[u8; 32] = &[0u8; 32]; // Placeholder - use proper key derivation

pub struct KeyVault {
    conn: Arc<Connection>,
}

impl KeyVault {
    /// Open or create key vault database
    pub fn open<P: AsRef<Path>>(db_path: P) -> Result<Self> {
        let conn = Connection::open(db_path.as_ref()).context("Failed to open SQLite database")?;

        // Create tables if not exist
        conn.execute(
            "CREATE TABLE IF NOT EXISTS api_keys (
                id TEXT PRIMARY KEY,
                provider TEXT NOT NULL,
                key_encrypted BLOB NOT NULL,
                quota_limit REAL,
                quota_used REAL DEFAULT 0.0,
                rate_limit_rpm INTEGER,
                is_active INTEGER DEFAULT 1,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS ranker_metrics (
                provider TEXT NOT NULL,
                model TEXT NOT NULL,
                key_id TEXT NOT NULL,
                latency_ema REAL DEFAULT 0.0,
                cost_per_token REAL DEFAULT 0.0,
                capability_flags INTEGER DEFAULT 0,
                last_updated INTEGER NOT NULL,
                PRIMARY KEY (provider, model, key_id)
            )",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(conn),
        })
    }

    /// Add a new API key
    pub fn add_key(&self, key: ApiKey) -> Result<()> {
        let encrypted = self.encrypt_key(&key.key)?;

        self.conn.execute(
            "INSERT OR REPLACE INTO api_keys 
             (id, provider, key_encrypted, quota_limit, quota_used, rate_limit_rpm, is_active, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                key.id,
                key.provider,
                encrypted,
                key.quota_limit,
                key.quota_used,
                key.rate_limit_rpm,
                if key.is_active { 1 } else { 0 },
                key.created_at,
                chrono::Utc::now().timestamp(),
            ],
        )?;

        Ok(())
    }

    /// Get all active keys for a provider
    pub fn get_keys_for_provider(&self, provider: &str) -> Result<Vec<ApiKey>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, provider, key_encrypted, quota_limit, quota_used, 
                    rate_limit_rpm, is_active, created_at
             FROM api_keys
             WHERE provider = ?1 AND is_active = 1",
        )?;

        let keys = stmt.query_map(params![provider], |row| {
            let encrypted: Vec<u8> = row.get(2)?;
            let decrypted = self.decrypt_key(&encrypted)?;

            Ok(ApiKey {
                id: row.get(0)?,
                provider: row.get(1)?,
                key: decrypted,
                quota_limit: row.get(3)?,
                quota_used: row.get(4)?,
                rate_limit_rpm: row.get(5)?,
                is_active: row.get::<_, i32>(6)? == 1,
                created_at: row.get(7)?,
            })
        })?;

        keys.collect::<Result<Vec<_>, _>>()
    }

    /// Get a specific key by ID
    pub fn get_key(&self, key_id: &str) -> Result<Option<ApiKey>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, provider, key_encrypted, quota_limit, quota_used,
                    rate_limit_rpm, is_active, created_at
             FROM api_keys
             WHERE id = ?1",
        )?;

        let key = stmt.query_row(params![key_id], |row| {
            let encrypted: Vec<u8> = row.get(2)?;
            let decrypted = self.decrypt_key(&encrypted)?;

            Ok(ApiKey {
                id: row.get(0)?,
                provider: row.get(1)?,
                key: decrypted,
                quota_limit: row.get(3)?,
                quota_used: row.get(4)?,
                rate_limit_rpm: row.get(5)?,
                is_active: row.get::<_, i32>(6)? == 1,
                created_at: row.get(7)?,
            })
        });

        match key {
            Ok(k) => Ok(Some(k)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Update key quota usage
    pub fn update_quota_usage(&self, key_id: &str, tokens_used: f64) -> Result<()> {
        self.conn.execute(
            "UPDATE api_keys 
             SET quota_used = quota_used + ?1, updated_at = ?2
             WHERE id = ?3",
            params![tokens_used, chrono::Utc::now().timestamp(), key_id],
        )?;

        Ok(())
    }

    /// Update ranker metrics for a key
    pub fn update_ranker_metrics(
        &self,
        provider: &str,
        model: &str,
        key_id: &str,
        latency_ms: f64,
        cost_per_token: f64,
    ) -> Result<()> {
        // Get current EMA latency
        let current_latency: Option<f64> = self
            .conn
            .query_row(
                "SELECT latency_ema FROM ranker_metrics 
             WHERE provider = ?1 AND model = ?2 AND key_id = ?3",
                params![provider, model, key_id],
                |row| row.get(0),
            )
            .unwrap_or(None);

        // Calculate new EMA (alpha = 0.3)
        let new_latency = match current_latency {
            Some(current) => current * 0.7 + latency_ms * 0.3,
            None => latency_ms,
        };

        self.conn.execute(
            "INSERT OR REPLACE INTO ranker_metrics
             (provider, model, key_id, latency_ema, cost_per_token, last_updated)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                provider,
                model,
                key_id,
                new_latency,
                cost_per_token,
                chrono::Utc::now().timestamp(),
            ],
        )?;

        Ok(())
    }

    /// Get ranker metrics for a key
    pub fn get_ranker_metrics(
        &self,
        provider: &str,
        model: &str,
        key_id: &str,
    ) -> Result<Option<(f64, f64)>> {
        let metrics = self.conn.query_row(
            "SELECT latency_ema, cost_per_token FROM ranker_metrics
             WHERE provider = ?1 AND model = ?2 AND key_id = ?3",
            params![provider, model, key_id],
            |row| Ok((row.get::<_, f64>(0)?, row.get::<_, f64>(1)?)),
        );

        match metrics {
            Ok(m) => Ok(Some(m)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// Encrypt API key (placeholder - use proper encryption in production)
    fn encrypt_key(&self, key: &str) -> Result<Vec<u8>> {
        // TODO: Use proper encryption (e.g., ChaCha20-Poly1305)
        // For now, just return bytes (insecure!)
        Ok(key.as_bytes().to_vec())
    }

    /// Decrypt API key (placeholder - use proper decryption in production)
    fn decrypt_key(&self, encrypted: &[u8]) -> Result<String> {
        // TODO: Use proper decryption
        // For now, just convert bytes to string
        String::from_utf8(encrypted.to_vec()).context("Failed to decrypt key")
    }

    /// Delete a key
    pub fn delete_key(&self, key_id: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM api_keys WHERE id = ?1", params![key_id])?;

        Ok(())
    }

    /// List all providers
    pub fn list_providers(&self) -> Result<Vec<String>> {
        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT provider FROM api_keys WHERE is_active = 1")?;

        let providers = stmt.query_map([], |row| row.get::<_, String>(0))?;
        providers.collect::<Result<Vec<_>, _>>()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_key_vault_basic() -> Result<()> {
        let dir = tempdir()?;
        let db_path = dir.path().join("test.db");

        let vault = KeyVault::open(&db_path)?;

        // Add a key
        let key = ApiKey {
            id: "test-key-1".to_string(),
            provider: "anthropic".to_string(),
            key: "sk-test-123".to_string(),
            quota_limit: Some(100.0),
            quota_used: 0.0,
            rate_limit_rpm: Some(1000),
            is_active: true,
            created_at: chrono::Utc::now().timestamp(),
        };

        vault.add_key(key.clone())?;

        // Retrieve the key
        let retrieved = vault.get_key("test-key-1")?;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().provider, "anthropic");

        // List providers
        let providers = vault.list_providers()?;
        assert_eq!(providers, vec!["anthropic"]);

        Ok(())
    }
}
