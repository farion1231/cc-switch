#![allow(non_snake_case)]

use crate::bridges::plugin as plugin_bridge;
use crate::config::ConfigStatus;

#[tauri::command]
pub async fn get_claude_plugin_status() -> Result<ConfigStatus, String> {
    plugin_bridge::get_status().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn read_claude_plugin_config() -> Result<Option<String>, String> {
    plugin_bridge::read_config().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_claude_plugin_config(official: bool) -> Result<bool, String> {
    plugin_bridge::apply_config(official).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn is_claude_plugin_applied() -> Result<bool, String> {
    plugin_bridge::is_applied().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn apply_claude_onboarding_skip() -> Result<bool, String> {
    plugin_bridge::apply_onboarding_skip().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_claude_onboarding_skip() -> Result<bool, String> {
    plugin_bridge::clear_onboarding_skip().map_err(|e| e.to_string())
}
