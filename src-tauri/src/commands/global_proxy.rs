//! 全局出站代理相关命令

use crate::bridges::global_proxy as global_proxy_bridge;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub fn get_global_proxy_url(_state: State<'_, AppState>) -> Result<Option<String>, String> {
    global_proxy_bridge::get_proxy_url().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_global_proxy_url(_state: State<'_, AppState>, url: String) -> Result<(), String> {
    global_proxy_bridge::set_proxy_url(&url).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_proxy_url(url: String) -> Result<cc_switch_core::ProxyTestResult, String> {
    global_proxy_bridge::test_proxy_url(&url)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_upstream_proxy_status() -> Result<cc_switch_core::UpstreamProxyStatus, String> {
    global_proxy_bridge::get_status().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn scan_local_proxies() -> Result<Vec<cc_switch_core::DetectedProxy>, String> {
    global_proxy_bridge::scan_local_proxies()
        .await
        .map_err(|e| e.to_string())
}
