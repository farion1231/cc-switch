use crate::services::pi_state::{PiCurrentState, PiStateService};
use crate::session_manager::providers::pi::PiSessionDiscovery;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_pi_current_state(state: State<'_, AppState>) -> Result<PiCurrentState, String> {
    PiStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn get_pi_session_discovery() -> PiSessionDiscovery {
    crate::session_manager::providers::pi::session_discovery()
}
