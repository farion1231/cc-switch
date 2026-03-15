#![allow(non_snake_case)]

/// Check Tool Search patch status for the active Claude Code installation
#[tauri::command]
pub async fn check_toolsearch_status() -> Result<crate::toolsearch_patch::ToolSearchStatus, String>
{
    crate::toolsearch_patch::check_toolsearch_status().map_err(|e| e.to_string())
}

/// Apply Tool Search patch (bypass domain restriction) to the active installation
#[tauri::command]
pub async fn apply_toolsearch_patch() -> Result<Vec<crate::toolsearch_patch::PatchResult>, String> {
    crate::toolsearch_patch::apply_toolsearch_patch().map_err(|e| e.to_string())
}

/// Restore Tool Search patch (re-enable domain restriction) for the active installation
#[tauri::command]
pub async fn restore_toolsearch_patch() -> Result<Vec<crate::toolsearch_patch::PatchResult>, String>
{
    crate::toolsearch_patch::restore_toolsearch_patch().map_err(|e| e.to_string())
}
