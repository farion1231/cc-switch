use crate::provider::UsageScript;
use crate::services::ohmypi_state::{OhMyPiCurrentState, OhMyPiStateService};
use crate::services::ProviderService;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_ohmypi_current_state(
    state: State<'_, AppState>,
) -> Result<OhMyPiCurrentState, String> {
    OhMyPiStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn update_ohmypi_provider_usage_script(
    state: State<'_, AppState>,
    id: String,
    #[allow(non_snake_case)] usageScript: UsageScript,
) -> Result<bool, String> {
    ProviderService::update_ohmypi_usage_script(state.inner(), &id, usageScript)
        .map_err(|error| error.to_string())
}
