//! Keys follow pubkey - simple identity model
//!
//! known_peers.json maps pubkey fingerprints to allowed providers.
//! On connect, keys are unlocked for that pubkey's session.

use anyhow::{Result, Context};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use std::fs;

/// Peer entry - which providers this pubkey can access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerEntry {
    /// Provider names this pubkey can unlock
    pub providers: Vec<String>,
    /// When this pubkey was added
    pub added: String,
    /// Optional: quota limits per provider
    #[serde(default)]
    pub quotas: HashMap<String, f64>,
}

/// Known peers store - just a JSON file
pub struct KnownPeers {
    path: PathBuf,
    peers: HashMap<String, PeerEntry>, // fingerprint -> entry
}

impl KnownPeers {
    /// Open known_peers.json
    pub fn open(base_dir: &PathBuf) -> Result<Self> {
        let path = base_dir.join("known_peers.json");
        
        let peers = if path.exists() {
            let content = fs::read_to_string(&path)
                .context("Failed to read known_peers.json")?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            HashMap::new()
        };
        
        Ok(Self { path, peers })
    }
    
    /// Add a pubkey with provider access
    pub fn add(&mut self, fingerprint: &str, providers: Vec<String>) -> Result<()> {
        let entry = PeerEntry {
            providers,
            added: chrono::Utc::now().to_rfc3339(),
            quotas: HashMap::new(),
        };
        
        self.peers.insert(fingerprint.to_string(), entry);
        self.save()
    }
    
    /// Remove a pubkey
    pub fn remove(&mut self, fingerprint: &str) -> Result<bool> {
        if self.peers.remove(fingerprint).is_some() {
            self.save()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    
    /// Get providers for a pubkey
    pub fn get_providers(&self, fingerprint: &str) -> Option<&[String]> {
        Some(self.peers.get(fingerprint)?.providers.as_slice())
    }
    
    /// Check if pubkey is known
    pub fn contains(&self, fingerprint: &str) -> bool {
        self.peers.contains_key(fingerprint)
    }
    
    /// List all known pubkeys
    pub fn list(&self) -> &HashMap<String, PeerEntry> {
        &self.peers
    }
    
    /// Update quota usage
    pub fn update_quota(&mut self, fingerprint: &str, provider: &str, used: f64) -> Result<()> {
        if let Some(entry) = self.peers.get_mut(fingerprint) {
            entry.quotas.insert(provider.to_string(), used);
            self.save()?;
        }
        Ok(())
    }
    
    /// Save to disk
    fn save(&self) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.peers)
            .context("Failed to serialize known_peers")?;
        
        fs::write(&self.path, content)
            .context("Failed to write known_peers.json")?;
        
        Ok(())
    }
}

/// Example known_peers.json:
/// {
///   "SHA256:abc123...": {
///     "providers": ["openai", "anthropic"],
///     "added": "2026-02-23T10:00:00Z",
///     "quotas": {}
///   },
///   "SHA256:def456...": {
///     "providers": ["google"],
///     "added": "2026-02-23T11:00:00Z",
///     "quotas": {}
///   }
/// }
