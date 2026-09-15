use crate::services::omp_state::{OmpCurrentState, OmpStateService};
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_omp_current_state(state: State<'_, AppState>) -> Result<OmpCurrentState, String> {
    OmpStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_omp_config_path() -> Result<String, String> {
    crate::omp_config::get_omp_models_path()
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| error.to_string())
}
