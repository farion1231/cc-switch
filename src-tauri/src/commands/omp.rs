use crate::provider::UsageScript;
use crate::services::omp_state::{OmpCurrentState, OmpStateService};
use crate::services::ProviderService;
use crate::store::AppState;
use tauri::State;

#[tauri::command]
pub(crate) fn get_omp_current_state(state: State<'_, AppState>) -> Result<OmpCurrentState, String> {
    OmpStateService::current(state.inner()).map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) fn update_omp_provider_usage_script(
    state: State<'_, AppState>,
    id: String,
    #[allow(non_snake_case)] usageScript: UsageScript,
) -> Result<bool, String> {
    ProviderService::update_omp_usage_script(state.inner(), &id, usageScript)
        .map_err(|error| error.to_string())
}
