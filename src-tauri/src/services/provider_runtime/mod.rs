mod db_backed;
mod pi_backed;
mod types;

use crate::provider_runtime::{ProviderRuntimeApp, ProviderRuntimeCapabilities};
use crate::services::pi_provider::{PiProviderDraft, PiProviderPatchPreview};
use crate::{AppError, AppState, AppType};

pub use types::{PiProviderWriteResult, ProviderRuntimeProviders};

pub struct ProviderRuntimeService;

impl ProviderRuntimeService {
    pub fn capabilities(app: ProviderRuntimeApp) -> ProviderRuntimeCapabilities {
        app.capabilities()
    }

    pub fn list(
        state: Option<&AppState>,
        app: ProviderRuntimeApp,
    ) -> Result<ProviderRuntimeProviders, AppError> {
        match app {
            ProviderRuntimeApp::Pi => {
                pi_backed::PiBackedProviderRuntime::list().map(ProviderRuntimeProviders::Pi)
            }
            _ => {
                let state = state.ok_or_else(|| {
                    AppError::Message("DB-backed provider runtime requires app state".to_string())
                })?;
                let app_type = AppType::try_from(app)?;
                db_backed::DbBackedProviderRuntime::list(state, app_type)
                    .map(ProviderRuntimeProviders::Db)
            }
        }
    }

    pub fn current(state: Option<&AppState>, app: ProviderRuntimeApp) -> Result<String, AppError> {
        if !app.capabilities().has_current_provider {
            return Ok(String::new());
        }

        let state = state.ok_or_else(|| {
            AppError::Message("DB-backed provider runtime requires app state".to_string())
        })?;
        let app_type = AppType::try_from(app)?;
        db_backed::DbBackedProviderRuntime::current(state, app_type)
    }

    pub fn read_pi_models_meta() -> Result<String, AppError> {
        pi_backed::PiBackedProviderRuntime::read_models_meta()
    }

    pub fn preview_pi_provider_patch(
        draft: &PiProviderDraft,
    ) -> Result<PiProviderPatchPreview, AppError> {
        pi_backed::PiBackedProviderRuntime::preview_upsert(draft)
    }

    pub fn apply_pi_provider_patch(
        draft: &PiProviderDraft,
        expected_file_hash: &str,
    ) -> Result<PiProviderWriteResult, AppError> {
        pi_backed::PiBackedProviderRuntime::apply_upsert(draft, expected_file_hash)
    }

    pub fn delete_pi_provider(
        provider_id: &str,
        expected_file_hash: &str,
    ) -> Result<PiProviderWriteResult, AppError> {
        pi_backed::PiBackedProviderRuntime::delete(provider_id, expected_file_hash)
    }
}
