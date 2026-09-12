//! ZCode provider commands
//!
//! ZCode (dev.zcode.app) uses additive mode — users may already have providers
//! configured in `~/.zcode/v2/config.json`. These commands mirror the OpenCode /
//! Hermes import commands.

use crate::store::AppState;
use tauri::State;

// ============================================================================
// ZCode Provider Commands
// ============================================================================

/// Import providers from ZCode live config to database.
///
/// ZCode uses additive mode — users may already have providers
/// configured in `~/.zcode/v2/config.json`.
#[tauri::command]
pub fn import_zcode_providers_from_live(state: State<'_, AppState>) -> Result<usize, String> {
    crate::services::provider::import_zcode_providers_from_live(state.inner())
        .map_err(|e| e.to_string())
}

/// Get provider ids in the ZCode live config.
#[tauri::command]
pub fn get_zcode_live_provider_ids() -> Result<Vec<String>, String> {
    crate::zcode_config::get_providers()
        .map(|providers| providers.keys().cloned().collect())
        .map_err(|e| e.to_string())
}
