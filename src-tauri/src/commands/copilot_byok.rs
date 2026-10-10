use crate::copilot_byok::{self, CopilotByokGroup, CopilotByokState, CopilotByokSyncResult};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub fn copilot_byok_get_state(state: State<'_, AppState>) -> Result<CopilotByokState, String> {
    copilot_byok::get_state(state.db.as_ref()).map_err(Into::into)
}

#[tauri::command(rename_all = "camelCase")]
pub fn copilot_byok_set_targets(
    state: State<'_, AppState>,
    target_ids: Vec<String>,
) -> Result<CopilotByokState, String> {
    copilot_byok::set_targets(state.db.as_ref(), target_ids).map_err(Into::into)
}

#[tauri::command(rename_all = "camelCase")]
pub fn copilot_byok_add_custom_target(
    state: State<'_, AppState>,
    path: String,
    name: Option<String>,
) -> Result<CopilotByokState, String> {
    copilot_byok::add_custom_target(state.db.as_ref(), path, name).map_err(Into::into)
}

#[tauri::command(rename_all = "camelCase")]
pub fn copilot_byok_remove_custom_target(
    state: State<'_, AppState>,
    target_id: String,
) -> Result<CopilotByokState, String> {
    copilot_byok::remove_custom_target(state.db.as_ref(), &target_id).map_err(Into::into)
}

#[tauri::command(rename_all = "camelCase")]
pub fn copilot_byok_upsert_group(
    state: State<'_, AppState>,
    group: CopilotByokGroup,
) -> Result<CopilotByokState, String> {
    copilot_byok::upsert_group(state.db.as_ref(), group).map_err(Into::into)
}

#[tauri::command(rename_all = "camelCase")]
pub fn copilot_byok_delete_group(
    state: State<'_, AppState>,
    group_id: String,
) -> Result<CopilotByokState, String> {
    copilot_byok::delete_group(state.db.as_ref(), &group_id).map_err(Into::into)
}

#[tauri::command]
pub fn copilot_byok_sync(state: State<'_, AppState>) -> Result<CopilotByokSyncResult, String> {
    copilot_byok::sync(state.db.as_ref()).map_err(Into::into)
}

#[tauri::command(rename_all = "camelCase")]
pub fn copilot_byok_reorder_groups(
    state: State<'_, AppState>,
    group_ids: Vec<String>,
) -> Result<CopilotByokState, String> {
    copilot_byok::reorder_groups(state.db.as_ref(), group_ids).map_err(Into::into)
}

#[tauri::command(rename_all = "camelCase")]
pub fn copilot_byok_restore_backup(
    state: State<'_, AppState>,
    target_id: String,
) -> Result<bool, String> {
    copilot_byok::restore_backup(state.db.as_ref(), &target_id).map_err(Into::into)
}
