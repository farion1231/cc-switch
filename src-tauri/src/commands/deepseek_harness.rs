use crate::services::dsh_state::{DshCurrentState, DshStateService};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_dsh_current_state(state: State<'_, AppState>) -> Result<DshCurrentState, String> {
    DshStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn set_dsh_current_model(
    state: State<'_, AppState>,
    #[allow(non_snake_case)] providerId: String,
    #[allow(non_snake_case)] modelId: String,
) -> Result<bool, String> {
    crate::services::provider::set_dsh_current_model(state.inner(), &providerId, &modelId)
        .map(|_| true)
        .map_err(|error| error.to_string())
}
