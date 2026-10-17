use cc_command_api::command_api;

use crate::error::AppError;
use crate::provider::UsageScript;
use crate::services::pi_state::{PiCurrentState, PiStateService};
use crate::services::ProviderService;
use crate::session_manager::providers::pi::PiSessionDiscovery;
use crate::store::AppState;

#[command_api]
pub(crate) fn get_pi_current_state(state: &AppState) -> Result<PiCurrentState, AppError> {
    PiStateService::current(state).map_err(|error| AppError::Config(error.to_string()))
}

#[command_api]
pub(crate) fn update_pi_provider_usage_script(
    state: &AppState,
    id: String,
    #[allow(non_snake_case)] usageScript: UsageScript,
) -> Result<bool, AppError> {
    ProviderService::update_pi_usage_script(state, &id, usageScript)
        .map_err(|error| AppError::Config(error.to_string()))
}

#[command_api]
pub(crate) fn get_pi_session_discovery() -> PiSessionDiscovery {
    crate::session_manager::providers::pi::session_discovery()
}
