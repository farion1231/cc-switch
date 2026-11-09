//! 插件管理命令（逻辑见 `services::plugin_manager`）。
//!
//! 每条命令都会起一次 CLI 子进程，放到阻塞线程池里跑，不占 async 运行时。

use crate::services::plugin_manager::{
    self, PluginActionOutcome, PluginApp, PluginEntry, PluginMarketplace,
};
use crate::store::AppState;

async fn blocking<T, F>(f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("Plugin task failed: {e}"))?
}

#[tauri::command]
pub async fn list_installed_plugins(app: PluginApp) -> Result<Vec<PluginEntry>, String> {
    blocking(move || plugin_manager::list_installed(app)).await
}

#[tauri::command]
pub async fn list_plugin_catalog(app: PluginApp) -> Result<Vec<PluginEntry>, String> {
    blocking(move || plugin_manager::list_catalog(app)).await
}

#[tauri::command]
pub async fn list_plugin_marketplaces(app: PluginApp) -> Result<Vec<PluginMarketplace>, String> {
    blocking(move || plugin_manager::list_marketplaces(app)).await
}

#[tauri::command]
pub async fn install_plugin(app: PluginApp, id: String) -> Result<PluginActionOutcome, String> {
    blocking(move || plugin_manager::install_plugin(app, &id)).await
}

#[tauri::command]
pub async fn uninstall_plugin(
    state: tauri::State<'_, AppState>,
    app: PluginApp,
    id: String,
) -> Result<PluginActionOutcome, String> {
    let db = state.db.clone();
    blocking(move || plugin_manager::uninstall_plugin(&db, app, &id)).await
}

#[tauri::command]
pub async fn set_plugin_enabled(
    state: tauri::State<'_, AppState>,
    app: PluginApp,
    id: String,
    enabled: bool,
) -> Result<PluginActionOutcome, String> {
    let db = state.db.clone();
    blocking(move || plugin_manager::set_plugin_enabled(&db, app, &id, enabled)).await
}

#[tauri::command]
pub async fn update_plugin(
    state: tauri::State<'_, AppState>,
    app: PluginApp,
    id: String,
) -> Result<PluginActionOutcome, String> {
    let db = state.db.clone();
    blocking(move || plugin_manager::update_plugin(&db, app, &id)).await
}

#[tauri::command]
pub async fn add_plugin_marketplace(
    app: PluginApp,
    source: String,
) -> Result<PluginActionOutcome, String> {
    blocking(move || plugin_manager::add_marketplace(app, &source)).await
}

#[tauri::command]
pub async fn remove_plugin_marketplace(
    app: PluginApp,
    name: String,
) -> Result<PluginActionOutcome, String> {
    blocking(move || plugin_manager::remove_marketplace(app, &name)).await
}

#[tauri::command]
pub async fn refresh_plugin_marketplaces(app: PluginApp) -> Result<PluginActionOutcome, String> {
    blocking(move || plugin_manager::refresh_marketplaces(app)).await
}
