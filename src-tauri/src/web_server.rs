use once_cell::sync::Lazy;
use std::net::SocketAddr;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, Mutex};

use crate::web::{create_router, handlers::ws::WsState, models::app_state::AppState};

// Global state to track web server
static WEB_SERVER_HANDLE: Lazy<Mutex<Option<tokio::task::JoinHandle<()>>>> =
    Lazy::new(|| Mutex::const_new(None));

static WEB_SERVER_PORT: Lazy<Mutex<u16>> = Lazy::new(|| Mutex::const_new(3001));

static WEB_SERVER_BIND_ALL: Lazy<Mutex<bool>> = Lazy::new(|| Mutex::const_new(false));

/// Check if web server should auto-start from env var
pub fn should_auto_start_web_server() -> bool {
    std::env::var("CC_SWITCH_ENABLE_WEB")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Get web server port from env var or default
pub fn get_web_server_port() -> u16 {
    std::env::var("CC_SWITCH_WEB_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(3001)
}

/// Check if web server should bind to all interfaces (0.0.0.0) or just localhost (127.0.0.1)
pub fn should_bind_to_all_interfaces() -> bool {
    std::env::var("CC_SWITCH_WEB_BIND_ALL")
        .ok()
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// Get the bind address based on bind_all flag
pub fn get_bind_address() -> [u8; 4] {
    if should_bind_to_all_interfaces() {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    }
}

fn bind_address_from_flag(bind_all: bool) -> [u8; 4] {
    if bind_all {
        [0, 0, 0, 0]
    } else {
        [127, 0, 0, 1]
    }
}

/// Start the embedded web server
#[tauri::command]
pub async fn start_web_server(
    app: tauri::AppHandle,
    port: Option<u16>,
    bind_all: Option<bool>,
) -> Result<String, String> {
    // Set the resource directory for web assets
    if let Ok(resource_dir) = app.path().resource_dir() {
        std::env::set_var("TAURI_RESOURCE_DIR", resource_dir);
    }

    let mut handle_guard = WEB_SERVER_HANDLE.lock().await;

    if handle_guard.is_some() {
        return Err("Web server is already running".to_string());
    }

    let port = port.unwrap_or_else(get_web_server_port);
    let bind_all = bind_all.unwrap_or_else(should_bind_to_all_interfaces);
    let bind_addr = bind_address_from_flag(bind_all);
    let addr = SocketAddr::from((bind_addr, port));

    // Resolve DB path from app config directory
    let db_path = crate::config::get_app_config_dir()
        .join("cc-switch.db")
        .to_string_lossy()
        .to_string();

    // Create web app state
    let web_state = Arc::new(
        AppState::new(&db_path).map_err(|e| format!("Failed to create app state: {}", e))?,
    );

    // Create WebSocket broadcast channel
    let (tx, _rx) = broadcast::channel(100);
    let ws_state = Arc::new(WsState::new(tx));

    // Create router
    let app_router = create_router(web_state, ws_state);

    // Bind to address
    let listener = TcpListener::bind(addr)
        .await
        .map_err(|e| format!("Failed to bind to {}: {}", addr, e))?;

    // Store state for later reference
    *WEB_SERVER_PORT.lock().await = port;
    *WEB_SERVER_BIND_ALL.lock().await = bind_all;

    // Start server in background task
    let handle = tokio::spawn(async move {
        log::info!("Web server started on http://{}", addr);

        if let Err(e) = axum::serve(listener, app_router).await {
            log::error!("Web server error: {}", e);
        }

        log::info!("Web server stopped");
    });

    *handle_guard = Some(handle);

    // Determine display URL based on bind address
    let display_host = if bind_all { "0.0.0.0" } else { "localhost" };

    // Emit event to frontend
    let _ = app.emit(
        "web-server-started",
        serde_json::json!({
            "url": format!("http://{}:{}", display_host, port),
            "port": port,
            "bindAll": bind_all,
        }),
    );

    Ok(format!("http://{}:{}", display_host, port))
}

/// Stop the embedded web server
#[tauri::command]
pub async fn stop_web_server(app: tauri::AppHandle) -> Result<(), String> {
    let mut handle_guard = WEB_SERVER_HANDLE.lock().await;

    if let Some(handle) = handle_guard.take() {
        handle.abort();

        // Emit event to frontend
        let _ = app.emit("web-server-stopped", ());

        log::info!("Web server stopped");
        Ok(())
    } else {
        Err("Web server is not running".to_string())
    }
}

/// Check if web server is running
#[tauri::command]
pub async fn is_web_server_running() -> bool {
    WEB_SERVER_HANDLE.lock().await.is_some()
}

/// Get web server URL if running
#[tauri::command]
pub async fn get_web_server_url() -> Option<String> {
    if WEB_SERVER_HANDLE.lock().await.is_some() {
        let port = *WEB_SERVER_PORT.lock().await;
        let bind_all = *WEB_SERVER_BIND_ALL.lock().await;
        let host = if bind_all { "0.0.0.0" } else { "localhost" };
        Some(format!("http://{}:{}", host, port))
    } else {
        None
    }
}

/// Auto-start web server if env var is set
pub async fn auto_start_web_server(app: tauri::AppHandle) -> Result<(), String> {
    if should_auto_start_web_server() {
        log::info!("Auto-starting web server (CC_SWITCH_ENABLE_WEB is set)");
        start_web_server(app, None, None).await.map(|_| ())
    } else {
        Ok(())
    }
}

/// Check if web server is configured to bind to all interfaces
#[tauri::command]
pub async fn is_web_server_bind_all() -> bool {
    *WEB_SERVER_BIND_ALL.lock().await
}

/// Generate a JWT token for web access
#[tauri::command]
pub async fn generate_web_token() -> Result<String, String> {
    crate::web::middleware::auth::generate_token("admin")
        .map_err(|e| format!("Failed to generate token: {}", e))
}

/// Get web server configuration
#[tauri::command]
pub async fn get_web_server_config() -> serde_json::Value {
    let running = WEB_SERVER_HANDLE.lock().await.is_some();
    let port = *WEB_SERVER_PORT.lock().await;
    let bind_all = *WEB_SERVER_BIND_ALL.lock().await;
    let host = if bind_all { "0.0.0.0" } else { "localhost" };

    serde_json::json!({
        "running": running,
        "port": port,
        "bindAll": bind_all,
        "url": if running { Some(format!("http://{}:{}", host, port)) } else { None::<String> },
        "defaultPort": get_web_server_port(),
        "defaultBindAll": should_bind_to_all_interfaces(),
    })
}
