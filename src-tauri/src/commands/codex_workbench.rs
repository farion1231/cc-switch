//! Codex 工作台 Tauri 命令

use crate::error::AppError;
use crate::services::codex_workbench::{self, CodexWorkbenchStatus};
use crate::settings::CodexWorkbenchSettings;
use crate::store::AppState;
use std::sync::Arc;
use tauri::State;

#[tauri::command]
pub async fn get_codex_workbench_status(
    state: State<'_, Arc<AppState>>,
) -> Result<CodexWorkbenchStatus, AppError> {
    codex_workbench::get_status(state.inner().clone()).await
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
