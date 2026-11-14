//! 「我的图标」：用户上传过的自定义图标库

use tauri::State;

use super::run_db_task;
use crate::database::CustomIconEntry;
use crate::services::provider::icon::{self, AddLibraryIconResult};
use crate::store::AppState;

#[tauri::command]
pub async fn get_custom_icons(state: State<'_, AppState>) -> Result<Vec<CustomIconEntry>, String> {
    run_db_task(&state, |state| {
        icon::list_library_icons(&state.db).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn add_custom_icon(
    state: State<'_, AppState>,
    name: String,
    data: String,
) -> Result<AddLibraryIconResult, String> {
    run_db_task(&state, move |state| {
        icon::add_library_icon(&state.db, &name, &data).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn rename_custom_icon(
    state: State<'_, AppState>,
    id: String,
    name: String,
) -> Result<CustomIconEntry, String> {
    run_db_task(&state, move |state| {
        icon::rename_library_icon(&state.db, &id, &name).map_err(|e| e.to_string())
    })
    .await
}

#[tauri::command]
pub async fn delete_custom_icon(state: State<'_, AppState>, id: String) -> Result<(), String> {
    run_db_task(&state, move |state| {
        icon::delete_library_icon(&state.db, &id).map_err(|e| e.to_string())
    })
    .await
}
