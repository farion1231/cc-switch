use tauri::{command, AppHandle};

async fn apply_remote_config(
    app: &AppHandle,
    port: u16,
    tailscale_enabled: bool,
    action: &str,
) -> Result<String, String> {
    log::info!(
        "[Remote] {action}_remote_server called with port: {port}, tailscale: {tailscale_enabled}"
    );

    let config = crate::remote::RemoteConfig {
        enabled: true,
        port,
        tailscale_enabled,
    };

    let urls = crate::remote::start_remote(app, config).await?;
    log::info!("[Remote] Server {action}ed on URLs: {:?}", urls);
    Ok(format!("Remote server {action}ed on: {}", urls.join(", ")))
}

#[command]
pub async fn start_remote_server(
    app: AppHandle,
    port: u16,
    tailscale_enabled: bool,
) -> Result<String, String> {
    apply_remote_config(&app, port, tailscale_enabled, "start").await
}

#[command]
pub async fn stop_remote_server(app: AppHandle) -> Result<String, String> {
    crate::remote::stop_remote(&app).await?;
    Ok("Remote server stopped".to_string())
}

/// Restart the remote server without broadcasting SSE shutdown.
/// Used for Tailscale toggle and port changes to avoid "closed" page on browsers.
#[command]
pub async fn restart_remote_server(
    app: AppHandle,
    port: u16,
    tailscale_enabled: bool,
) -> Result<String, String> {
    apply_remote_config(&app, port, tailscale_enabled, "restart").await
}

#[command]
pub fn check_tailscale_available() -> bool {
    crate::remote::is_tailscale_available()
}

#[command]
pub fn get_tailscale_ip() -> Option<String> {
    crate::remote::get_tailscale_ip()
}
