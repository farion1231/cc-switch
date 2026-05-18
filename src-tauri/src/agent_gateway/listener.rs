use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::proxy::server::ProxyServer;
use crate::proxy::types::ProxyConfig;
use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

static AGENT_LISTENERS: Lazy<Mutex<HashMap<String, Arc<ProxyServer>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));

pub async fn start_agent_listener(
    db: Arc<Database>,
    agent_id: &str,
    provider: Provider,
    port: u16,
    app_handle: Option<tauri::AppHandle>,
) -> Result<(), AppError> {
    let mut listeners = AGENT_LISTENERS.lock().await;
    if listeners.contains_key(agent_id) {
        return Ok(());
    }

    let config = ProxyConfig {
        listen_address: "127.0.0.1".to_string(),
        listen_port: port,
        max_retries: 3,
        request_timeout: 600,
        enable_logging: true,
        live_takeover_active: false,
        streaming_first_byte_timeout: 60,
        streaming_idle_timeout: 120,
        non_streaming_timeout: 600,
    };
    let server = Arc::new(ProxyServer::new_forced_provider_snapshot(
        config,
        db,
        app_handle,
        "claude".to_string(),
        provider,
    ));
    server.start().await.map_err(|e| {
        AppError::Message(format!(
            "AGENT_LISTENER_FAILED: failed to start listener on port {port}: {e}"
        ))
    })?;
    listeners.insert(agent_id.to_string(), server);
    Ok(())
}

pub async fn stop_agent_listener(agent_id: &str) -> Result<(), AppError> {
    let server = {
        let mut listeners = AGENT_LISTENERS.lock().await;
        listeners.remove(agent_id)
    };
    if let Some(server) = server {
        server.stop().await.map_err(|e| {
            AppError::Message(format!(
                "AGENT_LISTENER_STOP_FAILED: failed to stop listener for {agent_id}: {e}"
            ))
        })?;
    }
    Ok(())
}

pub async fn stop_all_agent_listeners() {
    let servers = {
        let mut listeners = AGENT_LISTENERS.lock().await;
        listeners
            .drain()
            .map(|(_, server)| server)
            .collect::<Vec<_>>()
    };
    for server in servers {
        let _ = server.stop().await;
    }
}
