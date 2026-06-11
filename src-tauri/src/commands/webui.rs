use std::sync::Arc;

use serde::Serialize;
use tauri::State;

use crate::webui::WebUiServer;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WebUiStatus {
    pub running: bool,
    pub address: Option<String>,
    pub enabled: bool,
    pub port: u16,
    pub host: String,
    pub token_set: bool,
}

#[tauri::command]
pub async fn get_webui_status(webui: State<'_, Arc<WebUiServer>>) -> Result<WebUiStatus, String> {
    let settings = crate::settings::get_settings();
    let running = webui.is_running().await;
    let address = if running {
        webui.public_address().await
    } else {
        None
    };
    Ok(WebUiStatus {
        running,
        address,
        enabled: settings.webui_enabled,
        port: settings.webui_port,
        host: settings.webui_host.clone(),
        token_set: settings
            .webui_token
            .as_ref()
            .map(|t| !t.trim().is_empty())
            .unwrap_or(false),
    })
}

#[tauri::command]
pub async fn start_webui_server(webui: State<'_, Arc<WebUiServer>>) -> Result<String, String> {
    let addr = webui.start_from_settings().await?;
    Ok(format!("http://{addr}"))
}

#[tauri::command]
pub async fn stop_webui_server(webui: State<'_, Arc<WebUiServer>>) -> Result<(), String> {
    webui.stop().await
}

#[tauri::command]
pub async fn restart_webui_server(webui: State<'_, Arc<WebUiServer>>) -> Result<String, String> {
    // Stop if running
    let _ = webui.stop().await;
    // Start with current settings
    let addr = webui.start_from_settings().await?;
    Ok(format!("http://{addr}"))
}
