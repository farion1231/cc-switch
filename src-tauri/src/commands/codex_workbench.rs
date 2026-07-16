//! Codex 工作台 Tauri 命令

use crate::error::AppError;
use crate::services::codex_runtime::LaunchEnhancedCodexResult;
use crate::services::codex_workbench::{self, CodexWorkbenchStatus};
use crate::settings::CodexWorkbenchSettings;
use crate::store::AppState;
use tauri::State;

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
pub fn update_codex_workbench_settings(
    settings: CodexWorkbenchSettings,
) -> Result<(), AppError> {
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
