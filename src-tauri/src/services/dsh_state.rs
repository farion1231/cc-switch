//! Read-only DeepSeek Harness native state for advisory UI.

use crate::error::AppError;
use crate::store::AppState;
use serde::Serialize;

const DSH_APP: &str = "deepseek-harness";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DshCurrentState {
    pub provider_ids: Vec<String>,
    pub current_provider_id: Option<String>,
    pub current_model: Option<String>,
}

pub(crate) struct DshStateService;

impl DshStateService {
    pub(crate) fn current(state: &AppState) -> Result<DshCurrentState, AppError> {
        let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(DSH_APP));
        let native = crate::deepseek_harness_config::read_native_state()?;
        Ok(DshCurrentState {
            provider_ids: native.providers.keys().cloned().collect(),
            current_provider_id: native.current_provider,
            current_model: native.current_model,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use std::sync::Arc;

    fn with_temp_home(test: impl FnOnce(&std::path::Path)) {
        let directory = tempfile::tempdir().unwrap();
        let previous = std::env::var_os("DSH_HOME");
        std::env::set_var("DSH_HOME", directory.path());
        let result =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| test(directory.path())));
        match previous {
            Some(value) => std::env::set_var("DSH_HOME", value),
            None => std::env::remove_var("DSH_HOME"),
        }
        result.unwrap();
    }

    #[test]
    #[serial]
    fn exposes_native_providers_and_current_selection() {
        with_temp_home(|home| {
            std::fs::write(
                home.join("settings.yaml"),
                "llm-deepseek:\n  baseURL: \"https://api.deepseek.com\"\n  apiKeyEnv: DEEPSEEK_API_KEY\nllm-pi-ai:\n  providers:\n    k3:\n      baseURL: \"https://k3.example.com\"\nagent-default-model:\n  provider: k3\n  model: k3\n",
            )
            .unwrap();
            let state = AppState::new(Arc::new(
                crate::database::Database::memory().expect("memory db"),
            ));
            let current = DshStateService::current(&state).expect("state");
            assert!(current
                .provider_ids
                .contains(&"deepseek-official".to_string()));
            assert!(current.provider_ids.contains(&"k3".to_string()));
            assert_eq!(current.current_provider_id.as_deref(), Some("k3"));
            assert_eq!(current.current_model.as_deref(), Some("k3"));
        });
    }

    #[test]
    #[serial]
    fn empty_settings_yield_empty_state() {
        with_temp_home(|_home| {
            let state = AppState::new(Arc::new(
                crate::database::Database::memory().expect("memory db"),
            ));
            let current = DshStateService::current(&state).expect("state");
            assert!(current.provider_ids.is_empty());
            assert_eq!(current.current_provider_id, None);
        });
    }
}
