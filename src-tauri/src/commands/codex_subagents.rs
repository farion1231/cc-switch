#![allow(non_snake_case)]

use crate::codex_subagents::{self, CodexSubagentSettingsView};

#[tauri::command]
pub async fn get_codex_subagent_settings() -> Result<CodexSubagentSettingsView, String> {
    codex_subagents::get_settings_view().map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn save_codex_subagent_settings(
    model: Option<String>,
    reasoningEffort: Option<String>,
) -> Result<CodexSubagentSettingsView, String> {
    codex_subagents::save_settings(model, reasoningEffort).map_err(|error| error.to_string())
}
