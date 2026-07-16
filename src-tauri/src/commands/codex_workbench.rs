//! Codex 工作台 Tauri 命令

use crate::error::AppError;
use crate::services::codex_plugins::{self, MarketplaceResult, PluginCacheInfo};
use crate::services::codex_radar::{self, RadarResult};
use crate::services::codex_runtime::LaunchEnhancedCodexResult;
use crate::services::codex_scripts::{self, MarketIndex, ScriptInstallRequest, UserScriptInfo};
use crate::services::codex_workbench::{self, CodexWorkbenchStatus};
use crate::settings::{get_settings, CodexWorkbenchSettings};
use crate::store::AppState;
use std::path::PathBuf;
use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

#[tauri::command]
pub async fn get_codex_workbench_status(
    state: State<'_, AppState>,
) -> Result<CodexWorkbenchStatus, AppError> {
    codex_workbench::get_status(&state).await
}

#[tauri::command]
pub fn get_codex_workbench_settings() -> Result<CodexWorkbenchSettings, AppError> {
    Ok(codex_workbench::get_workbench_settings())
}

#[tauri::command]
pub fn update_codex_workbench_settings(settings: CodexWorkbenchSettings) -> Result<(), AppError> {
    codex_workbench::update_workbench_settings(settings)
}

#[tauri::command]
pub async fn launch_enhanced_codex(
    state: State<'_, AppState>,
) -> Result<LaunchEnhancedCodexResult, AppError> {
    codex_workbench::launch_enhanced(state.codex_runtime.as_ref()).await
}

#[tauri::command]
pub async fn reinject_codex_enhancements(
    state: State<'_, AppState>,
) -> Result<CodexWorkbenchStatus, AppError> {
    codex_workbench::reinject(state.codex_runtime.as_ref()).await?;
    codex_workbench::get_status(&state).await
}

// ---- user scripts / market ----

#[tauri::command]
pub fn list_codex_user_scripts() -> Result<Vec<UserScriptInfo>, AppError> {
    codex_scripts::list_scripts()
}

#[tauri::command]
pub fn set_codex_user_script_enabled(key: String, enabled: bool) -> Result<(), AppError> {
    codex_scripts::set_script_enabled(&key, enabled)
}

#[tauri::command]
pub fn delete_codex_user_script(key: String) -> Result<(), AppError> {
    codex_scripts::delete_user_script(&key)
}

#[tauri::command]
pub fn import_codex_user_script(
    source_path: String,
    key: Option<String>,
) -> Result<UserScriptInfo, AppError> {
    codex_scripts::import_local_script(PathBuf::from(source_path).as_path(), key.as_deref())
}

#[tauri::command]
pub fn open_codex_scripts_dir(handle: AppHandle) -> Result<(), AppError> {
    let dir = codex_scripts::scripts_dir_path()?;
    handle
        .opener()
        .open_path(dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|error| AppError::Message(format!("打开 Codex 脚本目录失败: {error}")))
}

#[tauri::command]
pub async fn refresh_codex_script_market() -> Result<MarketIndex, AppError> {
    let url = get_settings().codex_workbench.script_market_url;
    codex_scripts::refresh_market(&url).await
}

#[tauri::command]
pub fn get_codex_script_market_cache() -> Result<Option<MarketIndex>, AppError> {
    Ok(codex_scripts::cached_market())
}

#[tauri::command]
pub async fn install_codex_market_script(
    request: ScriptInstallRequest,
) -> Result<UserScriptInfo, AppError> {
    let url = get_settings().codex_workbench.script_market_url;
    codex_scripts::install_from_market(&url, &request).await
}

/// After any script mutation, reinject if enhanced Codex is running.
#[tauri::command]
pub async fn reinject_after_script_change(
    state: State<'_, AppState>,
) -> Result<CodexWorkbenchStatus, AppError> {
    let snap = state.codex_runtime.snapshot().await;
    if matches!(
        snap.state,
        crate::services::codex_runtime::CodexRuntimeState::Running
            | crate::services::codex_runtime::CodexRuntimeState::Injecting
    ) {
        let _ = codex_workbench::reinject(state.codex_runtime.as_ref()).await;
    }
    codex_workbench::get_status(&state).await
}

// --- Codex plugins (T10) ---

#[tauri::command]
pub fn get_codex_effective_home() -> Result<String, AppError> {
    Ok(codex_plugins::effective_codex_home().display().to_string())
}

#[tauri::command]
pub fn get_codex_plugin_marketplace_status() -> Result<MarketplaceResult, AppError> {
    let home = codex_plugins::effective_codex_home();
    Ok(codex_plugins::marketplace_status(&home))
}

#[tauri::command]
pub async fn initialize_codex_plugin_marketplace() -> Result<MarketplaceResult, AppError> {
    let home = codex_plugins::effective_codex_home();
    codex_plugins::initialize_curated_marketplace(&home).await
}

#[tauri::command]
pub fn list_codex_plugin_caches() -> Result<Vec<PluginCacheInfo>, AppError> {
    let home = codex_plugins::effective_codex_home();
    codex_plugins::list_plugin_caches(&home)
}

#[tauri::command]
pub fn refresh_codex_plugin_cache(plugin_id: String) -> Result<PluginCacheInfo, AppError> {
    let home = codex_plugins::effective_codex_home();
    codex_plugins::refresh_plugin_cache(&home, &plugin_id)
}

// --- Codex radar (T11) ---

#[tauri::command]
pub async fn get_codex_radar(refresh: Option<bool>) -> Result<RadarResult, AppError> {
    Ok(codex_radar::get_radar(refresh.unwrap_or(false)).await)
}
