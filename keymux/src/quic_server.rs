//! QUIC server using literbike's custom QUIC implementation
//!
//! This is the "soul" transport - user's own QUIC stack, not quinn/h3.
//! Simple binary protocol for agent authentication and key access.

use crate::acl_vault::AclKeyVault;
use crate::known_peers::KnownPeers;
use crate::ranker::Ranker;
use crate::router::AppState;
use crate::secret_lock::SecretLock;
use anyhow::{Context, Result};
use literbike::quic::{QuicServer, QuicError};
use log::{debug, error, info, warn};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// QUIC server configuration
#[derive(Debug, Clone)]
pub struct QuicConfig {
    pub port: u16,
    pub idle_timeout: u64,
    pub max_concurrent_streams: u64,
}

impl Default for QuicConfig {
    fn default() -> Self {
        Self {
            port: 8888,
            idle_timeout: 30,
            max_concurrent_streams: 100,
        }
    }
}

/// Active session - pubkey-authenticated connection with unlocked keys
#[derive(Debug, Clone)]
pub struct Session {
    pub pubkey_fingerprint: String,
    pub providers: Vec<String>,
    pub created_at: Instant,
    pub last_activity: Instant,
}

/// Session manager
pub struct SessionManager {
    sessions: Mutex<HashMap<String, Session>>,
    session_timeout: Duration,
}

impl SessionManager {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            session_timeout: Duration::from_secs(3600),
        }
    }

    pub fn create_session(&self, id: String, fingerprint: String, providers: Vec<String>) {
        let session = Session {
            pubkey_fingerprint: fingerprint,
            providers,
            created_at: Instant::now(),
            last_activity: Instant::now(),
        };
        self.sessions.lock().insert(id, session);
    }

    pub fn get_session(&self, id: &str) -> Option<Session> {
        self.sessions.lock().get(id).cloned()
    }

    pub fn touch_session(&self, id: &str) {
        if let Some(session) = self.sessions.lock().get_mut(id) {
            session.last_activity = Instant::now();
        }
    }

    pub fn remove_session(&self, id: &str) {
        self.sessions.lock().remove(id);
    }

    pub fn cleanup_expired(&self) {
        let mut sessions = self.sessions.lock();
        let now = Instant::now();
        sessions.retain(|_, s| now.duration_since(s.last_activity) < self.session_timeout);
    }
}

/// Binary protocol messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AgentMessage {
    Auth { pubkey_fingerprint: String, signature: Vec<u8> },
    ListProviders,
    GetKey { provider: String, session_id: String },
    ApiRequest { provider: String, method: String, path: String, body: Vec<u8> },
    Heartbeat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ServerMessage {
    AuthResult { session_id: Option<String>, error: Option<String> },
    Providers { providers: Vec<String> },
    Key { key: Option<String>, error: Option<String> },
    ApiResponse { status: u16, headers: Vec<(String, String)>, body: Vec<u8> },
    Error { message: String },
    Ok,
}

/// Start QUIC server with literbike
pub async fn start_quic_server(
    addr: SocketAddr,
    key_vault: Arc<AclKeyVault>,
    ranker: Arc<dyn Ranker>,
    base_dir: PathBuf,
    quic_config: Option<QuicConfig>,
) -> Result<()> {
    let config = quic_config.unwrap_or_default();

    info!("Starting literbike QUIC server on {}", addr);
    info!("  Max streams: {}", config.max_concurrent_streams);
    info!("  Idle timeout: {}s", config.idle_timeout);

    let known_peers = Arc::new(Mutex::new(KnownPeers::open(&base_dir)?));
    let secret_lock = Arc::new(Mutex::new(SecretLock::open(&base_dir)?));
    let sessions = Arc::new(SessionManager::new());

    let server = QuicServer::bind(addr)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to bind QUIC server: {}", e))?;

    info!("  literbike QUIC bound to {}", addr);

    server.start().await
        .map_err(|e| anyhow::anyhow!("Failed to start QUIC server: {}", e))?;

    Ok(())
}

/// Start auto protocol server (QUIC + TCP fallback)
pub async fn start_auto_server(
    addr: SocketAddr,
    key_vault: Arc<AclKeyVault>,
    ranker: Arc<dyn Ranker>,
    base_dir: PathBuf,
    quic_config: Option<QuicConfig>,
) -> Result<()> {
    let tcp_addr = SocketAddr::from(([0, 0, 0, 0], addr.port() + 1));

    info!("Starting auto protocol server...");
    info!("  QUIC (literbike): {} (UDP)", addr);
    info!("  TCP/HTTP: {} (TCP)", tcp_addr);

    let quic_key_vault = key_vault.clone();
    let quic_ranker = ranker.clone();
    let quic_base_dir = base_dir.clone();
    let quic_config_clone = quic_config.clone();
    
    let quic_handle = tokio::spawn(async move {
        start_quic_server(addr, quic_key_vault, quic_ranker, quic_base_dir, quic_config_clone).await
    });

    let tcp_key_vault = key_vault;
    let tcp_ranker = ranker;
    let tcp_handle = tokio::spawn(async move {
        crate::router::start_tcp_server(tcp_addr, tcp_key_vault, tcp_ranker).await
    });

    tokio::select! {
        result = quic_handle => {
            if let Err(e) = result? {
                error!("QUIC server failed: {}", e);
            }
        }
        result = tcp_handle => {
            if let Err(e) = result? {
                error!("TCP server failed: {}", e);
            }
        }
    }

    Ok(())
}

fn handle_agent_message(
    msg: AgentMessage,
    known_peers: &KnownPeers,
    secret_lock: &SecretLock,
    sessions: &SessionManager,
    key_vault: &AclKeyVault,
) -> ServerMessage {
    match msg {
        AgentMessage::Auth { pubkey_fingerprint, signature: _ } => {
            if let Some(providers) = known_peers.get_providers(&pubkey_fingerprint) {
                let session_id = uuid::Uuid::new_v4().to_string();
                sessions.create_session(
                    session_id.clone(),
                    pubkey_fingerprint.clone(),
                    providers.to_vec(),
                );
                info!("Authenticated session {} for pubkey {}", session_id, pubkey_fingerprint);
                ServerMessage::AuthResult {
                    session_id: Some(session_id),
                    error: None,
                }
            } else {
                warn!("Unknown pubkey: {}", pubkey_fingerprint);
                ServerMessage::AuthResult {
                    session_id: None,
                    error: Some("Unknown pubkey".to_string()),
                }
            }
        }

        AgentMessage::ListProviders => {
            let providers = key_vault.list_providers();
            ServerMessage::Providers { providers }
        }

        AgentMessage::GetKey { provider, session_id } => {
            if let Some(session) = sessions.get_session(&session_id) {
                if session.providers.contains(&provider) {
                    if let Ok(Some(key)) = secret_lock.unlock_secret(&provider, &session.pubkey_fingerprint) {
                        sessions.touch_session(&session_id);
                        ServerMessage::Key { key: Some(key), error: None }
                    } else {
                        ServerMessage::Key { key: None, error: Some("Failed to unlock key".to_string()) }
                    }
                } else {
                    ServerMessage::Key { key: None, error: Some("Provider not allowed for session".to_string()) }
                }
            } else {
                ServerMessage::Key { key: None, error: Some("Invalid session".to_string()) }
            }
        }

        AgentMessage::ApiRequest { provider, method, path, body } => {
            debug!("API request: {} {} from {}", method, path, provider);
            ServerMessage::Error { message: "API proxy not yet implemented".to_string() }
        }

        AgentMessage::Heartbeat => {
            ServerMessage::Ok
        }
    }
}
