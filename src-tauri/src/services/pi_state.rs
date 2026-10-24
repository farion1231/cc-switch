//! Read-only Pi provider membership and global default reference.

use crate::app_config::AppType;
use crate::error::AppError;
use crate::pi_config::{read_native_providers, read_pi_native_defaults};
use crate::store::AppState;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiCurrentState {
    pub enabled_provider_ids: Vec<String>,
    pub default_provider_id: Option<String>,
}

pub(crate) struct PiStateService;

impl PiStateService {
    pub(crate) fn current(state: &AppState) -> Result<PiCurrentState, AppError> {
        Self::current_for_app(state, &AppType::Pi)
    }

    pub(crate) fn current_for_app(
        state: &AppState,
        app: &AppType,
    ) -> Result<PiCurrentState, AppError> {
        let _guard =
            futures::executor::block_on(state.proxy_service.lock_switch_for_app(app.as_str()));
        let native = read_native_providers(app)?;
        let enabled_provider_ids = native.keys().cloned().collect::<Vec<_>>();
        // Only Pi exposes its global default as advisory UI. rpi membership must
        // never depend on Pi's settings.json (or its directory override).
        let default_provider_id = if app == &AppType::Pi {
            match read_pi_native_defaults() {
                Ok(defaults) => defaults.default_provider,
                Err(error) => {
                    log::warn!(
                        "Failed to read Pi global default provider for advisory UI: {error}"
                    );
                    None
                }
            }
        } else {
            None
        };
        Ok(PiCurrentState {
            enabled_provider_ids,
            default_provider_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::pi_config::test_support::TestAgentDir;
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;

    #[test]
    #[serial]
    fn rpi_state_uses_only_its_native_membership() {
        let _pi = TestAgentDir::new();
        let _rpi = crate::pi_config::test_support::TestRpiAgentDir::new();
        let state = AppState::new(Arc::new(Database::memory().expect("database")));
        let pi_path = crate::pi_config::models_path(&AppType::Pi).expect("Pi path");
        let rpi_path = crate::pi_config::models_path(&AppType::Rpi).expect("rpi path");
        for path in [&pi_path, &rpi_path] {
            fs::create_dir_all(path.parent().unwrap()).expect("agent directory");
        }
        fs::write(&pi_path, r#"{"providers":{"pi-only":{}}}"#).unwrap();
        fs::write(
            &rpi_path,
            r#"{"providers":{"native-rpi":{},"external-rpi":{"future":true}}}"#,
        )
        .unwrap();
        fs::write(
            crate::pi_config::get_pi_settings_path().unwrap(),
            "invalid JSON",
        )
        .unwrap();

        let current = PiStateService::current_for_app(&state, &AppType::Rpi).unwrap();
        assert_eq!(
            current.enabled_provider_ids,
            vec!["native-rpi", "external-rpi"]
        );
        assert_eq!(current.default_provider_id, None);
        assert_eq!(
            PiStateService::current(&state)
                .unwrap()
                .enabled_provider_ids,
            vec!["pi-only"]
        );

        // External changes must be observed, even without changing any database row.
        fs::write(&rpi_path, r#"{"providers":{}}"#).unwrap();
        assert!(PiStateService::current_for_app(&state, &AppType::Rpi)
            .unwrap()
            .enabled_provider_ids
            .is_empty());
        fs::write(&rpi_path, "invalid JSON").unwrap();
        assert!(PiStateService::current_for_app(&state, &AppType::Rpi).is_err());
        assert!(PiStateService::current(&state).is_ok());
    }

    #[test]
    #[serial]
    fn state_exposes_every_explicit_provider_node() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let models_path = crate::pi_config::get_pi_models_path().expect("models path");
        fs::create_dir_all(models_path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(
            models_path,
            r#"{
                "providers": {
                    "cc-switch-managed": {
                        "name": "Managed",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-a" }]
                    },
                    "native-oauth": {
                        "oauth": "example",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-b" }]
                    },
                    "anthropic": {},
                    "unsupported": {
                        "futureField": true
                    }
                }
            }"#,
        )
        .expect("write models");
        let settings_path = crate::pi_config::get_pi_settings_path().expect("settings path");
        fs::write(
            settings_path,
            r#"{"defaultProvider":"cc-switch-managed","defaultModel":"model-a"}"#,
        )
        .expect("write settings");

        let current = PiStateService::current(&state).expect("read state");
        assert_eq!(
            current.enabled_provider_ids,
            vec![
                "cc-switch-managed".to_string(),
                "native-oauth".to_string(),
                "anthropic".to_string(),
                "unsupported".to_string(),
            ]
        );
        assert_eq!(
            current.default_provider_id.as_deref(),
            Some("cc-switch-managed")
        );
    }

    #[test]
    #[serial]
    fn invalid_global_settings_do_not_hide_provider_membership() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let models_path = crate::pi_config::get_pi_models_path().expect("models path");
        fs::create_dir_all(models_path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(
            models_path,
            r#"{
                "providers": {
                    "cc-switch-managed": {
                        "name": "Managed",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-a" }]
                    }
                }
            }"#,
        )
        .expect("write models");
        let settings_path = crate::pi_config::get_pi_settings_path().expect("settings path");
        fs::write(settings_path, "[]").expect("write invalid settings");

        let current = PiStateService::current(&state).expect("read membership");
        assert_eq!(
            current.enabled_provider_ids,
            vec!["cc-switch-managed".to_string()]
        );
        assert_eq!(current.default_provider_id, None);
    }
}
