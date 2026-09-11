use crate::services::dsh_state::{DshCurrentState, DshStateService};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_dsh_current_state(state: State<'_, AppState>) -> Result<DshCurrentState, String> {
    DshStateService::current(state.inner()).map_err(|error| error.to_string())
}
