use crate::provider_runtime::{ProviderRuntimeApp, ProviderRuntimeCapabilities};
use crate::{AppError, AppState, AppType, ProviderService};

use super::types::DbProvidersMap;

pub struct DbBackedProviderRuntime;

impl DbBackedProviderRuntime {
    pub fn capabilities(app_type: AppType) -> ProviderRuntimeCapabilities {
        ProviderRuntimeApp::from(app_type).capabilities()
    }

    pub fn list(state: &AppState, app_type: AppType) -> Result<DbProvidersMap, AppError> {
        ProviderService::list(state, app_type)
    }

    pub fn current(state: &AppState, app_type: AppType) -> Result<String, AppError> {
        let caps = Self::capabilities(app_type.clone());
        if !caps.has_current_provider {
            return Ok(String::new());
        }
        ProviderService::current(state, app_type)
    }
}
