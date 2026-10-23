//! 「我的图标」：用户上传过的自定义图标库

use cc_command_api::command_api;

use crate::database::CustomIconEntry;
use crate::services::provider::icon::{self, AddLibraryIconResult};
use crate::store::AppState;

#[command_api]
pub fn get_custom_icons(state: State<'_, AppState>) -> Result<Vec<CustomIconEntry>, String> {
    icon::list_library_icons(&state.db).map_err(|e| e.to_string())
}

#[command_api]
pub fn add_custom_icon(
    state: State<'_, AppState>,
    name: String,
    data: String,
) -> Result<AddLibraryIconResult, String> {
    icon::add_library_icon(&state.db, &name, &data).map_err(|e| e.to_string())
}

#[command_api]
pub fn rename_custom_icon(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<CustomIconEntry, String> {
    icon::rename_library_icon(&state.db, &id, &name).map_err(|e| e.to_string())
}

#[command_api]
pub fn delete_custom_icon(state: State<'_, AppState>, id: String) -> Result<(), String> {
    icon::delete_library_icon(&state.db, &id).map_err(|e| e.to_string())
}
