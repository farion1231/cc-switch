//! Read-only OMP provider membership and default reference.

use crate::error::AppError;
use crate::store::AppState;
use serde::Serialize;

const OMP_APP: &str = "omp";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OmpCurrentState {
    pub enabled_provider_ids: Vec<String>,
    pub default_provider_id: Option<String>,
}

pub(crate) struct OmpStateService;

impl OmpStateService {
    pub(crate) fn current(state: &AppState) -> Result<OmpCurrentState, AppError> {
        let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(OMP_APP));
        let native = crate::omp_config::read_omp_native_providers()?;
        Ok(OmpCurrentState {
            enabled_provider_ids: native.keys().cloned().collect(),
            // OMP currently has no separate global default-provider setting.
            default_provider_id: None,
        })
    }
}
