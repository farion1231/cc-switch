//! Read-only Oh My Pi provider membership and global default reference.

use crate::error::AppError;
use crate::ohmypi_config::{
    provider_from_selector, read_ohmypi_default_model, read_ohmypi_native_providers,
};
use crate::store::AppState;
use serde::Serialize;

const OHMYPI_APP: &str = "ohmypi";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct OhMyPiCurrentState {
    pub enabled_provider_ids: Vec<String>,
    pub default_provider_id: Option<String>,
}

pub(crate) struct OhMyPiStateService;

impl OhMyPiStateService {
    pub(crate) fn current(state: &AppState) -> Result<OhMyPiCurrentState, AppError> {
        let _guard =
            futures::executor::block_on(state.proxy_service.lock_switch_for_app(OHMYPI_APP));
        let native = read_ohmypi_native_providers()?;
        let enabled_provider_ids = native.keys().cloned().collect::<Vec<_>>();
        // `modelRoles.default` is a `<provider>/<model>` selector. The provider
        // prefix is exposed even when the provider has no explicit node in
        // `models.yml` (built-in model activation).
        let default_provider_id = match read_ohmypi_default_model() {
            Ok(Some(selector)) => Some(provider_from_selector(&selector).to_string()),
            Ok(None) => None,
            Err(error) => {
                log::warn!("Failed to read Oh My Pi default model: {error}");
                None
            }
        };
        Ok(OhMyPiCurrentState {
            enabled_provider_ids,
            default_provider_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::ohmypi_config::test_support::TestAgentDir;
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;

    #[test]
    #[serial]
    fn state_exposes_explicit_providers_and_default() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let models_path = crate::ohmypi_config::get_ohmypi_models_path().expect("models path");
        fs::create_dir_all(models_path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(
            models_path,
            "providers:\n  managed:\n    baseUrl: https://api.example.com/v1\n    api: openai-completions\n    models:\n      - id: model-a\n  native:\n    baseUrl: https://native.example\n",
        )
        .expect("write models");
        let settings_path =
            crate::ohmypi_config::get_ohmypi_settings_path().expect("settings path");
        fs::write(settings_path, "modelRoles:\n  default: managed/model-a\n")
            .expect("write settings");

        let current = OhMyPiStateService::current(&state).expect("read state");
        assert_eq!(
            current.enabled_provider_ids,
            vec!["managed".to_string(), "native".to_string()]
        );
        assert_eq!(current.default_provider_id.as_deref(), Some("managed"));
    }

    #[test]
    #[serial]
    fn built_in_default_without_explicit_node_is_tolerated() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let models_path = crate::ohmypi_config::get_ohmypi_models_path().expect("models path");
        fs::create_dir_all(models_path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(models_path, "providers: {}\n").expect("write empty models");
        let settings_path =
            crate::ohmypi_config::get_ohmypi_settings_path().expect("settings path");
        fs::write(settings_path, "modelRoles:\n  default: openai/gpt-4o\n")
            .expect("write settings");

        let current = OhMyPiStateService::current(&state).expect("read state");
        assert!(current.enabled_provider_ids.is_empty());
        // modelRoles.default references a built-in provider with no explicit node.
        assert_eq!(current.default_provider_id.as_deref(), Some("openai"));
    }
}
