#![allow(non_snake_case)]

use crate::config::ConfigStatus;
use crate::services::plugin::{
    self, PluginActionResult, PluginApp, PluginClientStatus, PluginMarketplace, PluginScope,
    UnifiedPlugin,
};

#[tauri::command]
pub async fn get_plugin_client_statuses() -> Vec<PluginClientStatus> {
    let (codex, claude) = tokio::join!(
        plugin::client_status(PluginApp::Codex),
        plugin::client_status(PluginApp::Claude)
    );
    vec![codex, claude]
}

#[tauri::command]
pub async fn list_plugins(
    app: PluginApp,
    include_available: bool,
) -> Result<Vec<UnifiedPlugin>, String> {
    plugin::list_plugins(app, include_available).await
}

#[tauri::command]
pub async fn list_plugin_marketplaces(app: PluginApp) -> Result<Vec<PluginMarketplace>, String> {
    plugin::list_marketplaces(app).await
}

#[tauri::command]
pub async fn add_plugin_marketplace(
    app: PluginApp,
    source: String,
) -> Result<PluginActionResult, String> {
    plugin::add_marketplace(app, &source).await
}

#[tauri::command]
pub async fn refresh_plugin_marketplace(
    app: PluginApp,
    name: String,
) -> Result<PluginActionResult, String> {
    plugin::refresh_marketplace(app, &name).await
}

#[tauri::command]
pub async fn remove_plugin_marketplace(
    app: PluginApp,
    name: String,
) -> Result<PluginActionResult, String> {
    plugin::remove_marketplace(app, &name).await
}

#[tauri::command]
pub async fn install_plugin(
    app: PluginApp,
    plugin_id: String,
    scope: Option<PluginScope>,
    project_path: Option<String>,
) -> Result<PluginActionResult, String> {
    plugin::install_plugin(app, &plugin_id, scope, project_path.as_deref()).await
}

#[tauri::command]
pub async fn update_plugin(
    app: PluginApp,
    plugin_id: String,
    scope: Option<PluginScope>,
    project_path: Option<String>,
) -> Result<PluginActionResult, String> {
    plugin::update_plugin(app, &plugin_id, scope, project_path.as_deref()).await
}

#[tauri::command]
pub async fn set_plugin_enabled(
    app: PluginApp,
    plugin_id: String,
    enabled: bool,
    scope: Option<PluginScope>,
    project_path: Option<String>,
) -> Result<PluginActionResult, String> {
    plugin::set_plugin_enabled(app, &plugin_id, enabled, scope, project_path.as_deref()).await
}

#[tauri::command]
pub async fn uninstall_plugin(
    app: PluginApp,
    plugin_id: String,
    scope: Option<PluginScope>,
    project_path: Option<String>,
) -> Result<PluginActionResult, String> {
    plugin::uninstall_plugin(app, &plugin_id, scope, project_path.as_deref()).await
}

/// Claude 插件：获取 ~/.claude/config.json 状态
#[tauri::command]
pub async fn get_claude_plugin_status() -> Result<ConfigStatus, String> {
    crate::claude_plugin::claude_config_status()
        .map(|(exists, path)| ConfigStatus {
            exists,
            path: path.to_string_lossy().to_string(),
        })
        .map_err(|e| e.to_string())
}

/// Claude 插件：读取配置内容（若不存在返回 Ok(None)）
#[tauri::command]
pub async fn read_claude_plugin_config() -> Result<Option<String>, String> {
    crate::claude_plugin::read_claude_config().map_err(|e| e.to_string())
}

/// Claude 插件：写入/清除固定配置
#[tauri::command]
pub async fn apply_claude_plugin_config(official: bool) -> Result<bool, String> {
    if official {
        crate::claude_plugin::clear_claude_config().map_err(|e| e.to_string())
    } else {
        crate::claude_plugin::write_claude_config().map_err(|e| e.to_string())
    }
}

/// Claude 插件：检测是否已写入目标配置
#[tauri::command]
pub async fn is_claude_plugin_applied() -> Result<bool, String> {
    crate::claude_plugin::is_claude_config_applied().map_err(|e| e.to_string())
}

/// Claude Code：跳过初次安装确认（写入 ~/.claude.json 的 hasCompletedOnboarding=true）
#[tauri::command]
pub async fn apply_claude_onboarding_skip() -> Result<bool, String> {
    crate::claude_mcp::set_has_completed_onboarding().map_err(|e| e.to_string())
}

/// Claude Code：恢复初次安装确认（删除 ~/.claude.json 的 hasCompletedOnboarding 字段）
#[tauri::command]
pub async fn clear_claude_onboarding_skip() -> Result<bool, String> {
    crate::claude_mcp::clear_has_completed_onboarding().map_err(|e| e.to_string())
}
