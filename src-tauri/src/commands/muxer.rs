//! Tauri commands for ModelMux control
//!
//! Provides Tauri IPC commands for:
//! - Key management (add/remove/list keys)
//! - Muxer control (start/stop/status)
//! - Provider configuration
//! - Quota management
//! - Live metrics

use crate::acl_vault::{AclKeyVault, ProviderKey, Permissions};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Manager, Runtime, State};

/// Key vault state (shared across Tauri commands)
pub struct KeyVaultState(pub Arc<tokio::sync::RwLock<Option<Arc<AclKeyVault>>>>);

impl Default for KeyVaultState {
    fn default() -> Self {
        Self(Arc::new(tokio::sync::RwLock::new(None)))
    }
}

/// Muxer process state
pub struct MuxerState(pub Arc<tokio::sync::RwLock<Option<MuxerProcess>>>);

impl Default for MuxerState {
    fn default() -> Self {
        Self(Arc::new(tokio::sync::RwLock::new(None)))
    }
}

/// Running muxer process info
pub struct MuxerProcess {
    pub pid: u32,
    pub port: u16,
    pub protocol: String,
}

/// API key info (for Tauri frontend)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiKeyInfo {
    pub id: String,
    pub provider: String,
    pub quota_limit: Option<f64>,
    pub quota_used: f64,
    pub is_active: bool,
    pub created_at: i64,
}

/// Provider info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInfo {
    pub name: String,
    pub key_count: usize,
    pub total_quota_limit: Option<f64>,
    pub total_quota_used: f64,
}

/// Muxer status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MuxerStatus {
    pub is_running: bool,
    pub port: Option<u16>,
    pub protocol: Option<String>,
    pub pid: Option<u32>,
    pub uptime_seconds: Option<u64>,
}

/// Add API key request
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddKeyRequest {
    pub provider: String,
    pub key: String,
    pub quota_limit: Option<f64>,
    pub permissions: Option<u32>, // Octal (e.g., 0o600)
}

// ============================================================================
// Key Management Commands
// ============================================================================

/// Add a new API key
#[tauri::command]
pub async fn muxer_add_key(
    state: State<'_, KeyVaultState>,
    request: AddKeyRequest,
) -> Result<String, String> {
    let vault = state.0.read().await;
    let vault = vault.as_ref().ok_or("Key vault not initialized")?;
    
    let key_id = format!("key-{}-{}", request.provider, uuid::Uuid::new_v4().to_string()[..8]);
    
    let key = ProviderKey {
        id: key_id.clone(),
        provider: request.provider.clone(),
        key: request.key,
        quota_limit: request.quota_limit,
        quota_used: 0.0,
        is_active: true,
        permissions: Permissions::from_octal(request.permissions.unwrap_or(0o600)),
    };
    
    vault.add_key(key).map_err(|e| e.to_string())?;
    
    Ok(key_id)
}

/// Remove an API key
#[tauri::command]
pub async fn muxer_remove_key(
    state: State<'_, KeyVaultState>,
    key_id: String,
) -> Result<(), String> {
    let vault = state.0.read().await;
    let vault = vault.as_ref().ok_or("Key vault not initialized")?;
    
    // Note: AclKeyVault doesn't have remove_key yet, would need to be added
    // For now, just mark as inactive
    Err("Not implemented: use filesystem to remove keys".to_string())
}

/// List all API keys
#[tauri::command]
pub async fn muxer_list_keys(
    state: State<'_, KeyVaultState>,
    provider: Option<String>,
) -> Result<Vec<ApiKeyInfo>, String> {
    let vault = state.0.read().await;
    let vault = vault.as_ref().ok_or("Key vault not initialized")?;
    
    let keys = if let Some(provider) = provider {
        vault.get_keys_for_provider(&provider)
    } else {
        vault.list_keys().into_iter().cloned().collect()
    };
    
    Ok(keys.into_iter().map(|k| ApiKeyInfo {
        id: k.id,
        provider: k.provider,
        quota_limit: k.quota_limit,
        quota_used: k.quota_used,
        is_active: k.is_active,
        created_at: chrono::Utc::now().timestamp(),
    }).collect())
}

/// List all providers
#[tauri::command]
pub async fn muxer_list_providers(
    state: State<'_, KeyVaultState>,
) -> Result<Vec<ProviderInfo>, String> {
    let vault = state.0.read().await;
    let vault = vault.as_ref().ok_or("Key vault not initialized")?;
    
    let provider_names = vault.list_providers();
    
    let mut providers = Vec::new();
    for name in provider_names {
        let keys = vault.get_keys_for_provider(&name);
        let key_count = keys.len();
        let total_quota_limit: Option<f64> = keys.iter()
            .filter_map(|k| k.quota_limit)
            .sum::<f64>()
            .into();
        let total_quota_used: f64 = keys.iter()
            .map(|k| k.quota_used)
            .sum();
        
        providers.push(ProviderInfo {
            name,
            key_count,
            total_quota_limit,
            total_quota_used,
        });
    }
    
    Ok(providers)
}

/// Get key quota status
#[tauri::command]
pub async fn muxer_get_quota(
    state: State<'_, KeyVaultState>,
    key_id: String,
) -> Result<Option<(f64, f64)>, String> {
    let vault = state.0.read().await;
    let vault = vault.as_ref().ok_or("Key vault not initialized")?;
    
    if let Some(key) = vault.get_key(&key_id) {
        Ok(Some((key.quota_limit.unwrap_or(f64::INFINITY), key.quota_used)))
    } else {
        Ok(None)
    }
}

// ============================================================================
// Muxer Control Commands
// ============================================================================

/// Start ModelMux server
#[tauri::command]
pub async fn muxer_start(
    state: State<'_, MuxerState>,
    port: u16,
    protocol: String,
) -> Result<(), String> {
    let mut muxer = state.0.write().await;
    
    if muxer.is_some() {
        return Err("Muxer already running".to_string());
    }
    
    // In production, this would spawn the actual modelmux binary
    // For now, we'll just store the config
    *muxer = Some(MuxerProcess {
        pid: std::process::id(),
        port,
        protocol: protocol.clone(),
    });
    
    log::info!("Muxer started on port {} ({})", port, protocol);
    
    Ok(())
}

/// Stop ModelMux server
#[tauri::command]
pub async fn muxer_stop(
    state: State<'_, MuxerState>,
) -> Result<(), String> {
    let mut muxer = state.0.write().await;
    
    if let Some(process) = muxer.take() {
        log::info!("Muxer stopped (was running on port {})", process.port);
        Ok(())
    } else {
        Err("Muxer not running".to_string())
    }
}

/// Get Muxer status
#[tauri::command]
pub async fn muxer_status(
    state: State<'_, MuxerState>,
) -> Result<MuxerStatus, String> {
    let muxer = state.0.read().await;
    
    if let Some(process) = muxer.as_ref() {
        Ok(MuxerStatus {
            is_running: true,
            port: Some(process.port),
            protocol: Some(process.protocol.clone()),
            pid: Some(process.pid),
            uptime_seconds: Some(0), // TODO: track start time
        })
    } else {
        Ok(MuxerStatus {
            is_running: false,
            port: None,
            protocol: None,
            pid: None,
            uptime_seconds: None,
        })
    }
}

// ============================================================================
// Metrics Commands
// ============================================================================

/// Get LiteBike network metrics
#[tauri::command]
pub async fn muxer_get_litbike_metrics(
    state: State<'_, KeyVaultState>,
) -> Result<LiteBikeMetricsResponse, String> {
    // TODO: Integrate with LiteBike module
    Ok(LiteBikeMetricsResponse {
        best_interface: None,
        interfaces: vec![],
    })
}

/// LiteBike metrics response
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LiteBikeMetricsResponse {
    pub best_interface: Option<String>,
    pub interfaces: Vec<InterfaceMetrics>,
}

/// Interface metrics
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InterfaceMetrics {
    pub name: String,
    pub radio_type: String,
    pub signal_strength: Option<f64>,
    pub latency_ms: f64,
    pub packet_loss: f64,
    pub bandwidth_mbps: f64,
    pub quality_score: f64,
}

// ============================================================================
// Initialization
// ============================================================================

/// Initialize key vault state
pub fn init_key_vault<R: Runtime>(app: &AppHandle<R>) -> Result<()> {
    let base_dir = dirs::home_dir()
        .map(|h| h.join(".cc-switch"))
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    
    let vault = AclKeyVault::open(&base_dir)?;
    
    app.manage(KeyVaultState(Arc::new(tokio::sync::RwLock::new(Some(Arc::new(vault))))));
    app.manage(MuxerState::default());
    
    log::info!("ModelMux Tauri commands initialized");
    
    Ok(())
}
