//! Read-only Pi provider membership and global default reference.

use crate::error::AppError;
use crate::pi_config::{
    pi_provider_display_name, read_pi_auth_provider_ids, read_pi_native_defaults,
    read_pi_native_providers,
};
use crate::store::AppState;
use serde::Serialize;

const PI_APP: &str = "pi";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiLoginProvider {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiCurrentState {
    pub enabled_provider_ids: Vec<String>,
    pub default_provider_id: Option<String>,
    pub login_providers: Vec<PiLoginProvider>,
}

pub(crate) struct PiStateService;

impl PiStateService {
    pub(crate) fn current(state: &AppState) -> Result<PiCurrentState, AppError> {
        let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(PI_APP));
        let native = read_pi_native_providers()?;
        let enabled_provider_ids = native.keys().cloned().collect::<Vec<_>>();
        let default_provider_id = match read_pi_native_defaults() {
            Ok(defaults) => defaults.default_provider,
            Err(error) => {
                log::warn!("Failed to read Pi global default provider for advisory UI: {error}");
                None
            }
        };
        // `/login` providers are a runtime-only view. They never enter SQLite,
        // backups, cloud sync, or the explicit-provider CRUD paths. An enabled
        // models.json provider wins over its matching login projection.
        let login_providers = match read_pi_auth_provider_ids() {
            Ok(provider_ids) => provider_ids
                .into_iter()
                .filter(|id| !native.contains_key(id))
                .map(|id| PiLoginProvider {
                    name: pi_provider_display_name(&id).to_string(),
                    id,
                })
                .collect(),
            Err(error) => {
                log::warn!("Failed to read Pi auth provider ids for advisory UI: {error}");
                Vec::new()
            }
        };
        Ok(PiCurrentState {
            enabled_provider_ids,
            default_provider_id,
            login_providers,
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
    fn login_providers_are_runtime_only_and_explicit_models_win() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let agent_dir = crate::pi_config::get_pi_agent_dir().expect("agent directory");
        fs::create_dir_all(&agent_dir).expect("create agent directory");
        fs::write(
            agent_dir.join("models.json"),
            r#"{"providers":{"deepseek":{"name":"Explicit DeepSeek"},"5090":{"name":"5090"}}}"#,
        )
        .expect("write models");
        fs::write(
            agent_dir.join("auth.json"),
            r#"{
                "deepseek":{"type":"api_key","key":"sk-native"},
                "kimi-coding":{"type":"api_key","key":"sk-kimi"},
                "future-provider":{"oauth":{"access_token":"secret-token"}}
            }"#,
        )
        .expect("write auth");

        let current = PiStateService::current(&state).expect("read state");
        assert_eq!(
            current.enabled_provider_ids,
            vec!["deepseek".to_string(), "5090".to_string()]
        );
        assert_eq!(
            current.login_providers,
            vec![
                PiLoginProvider {
                    id: "kimi-coding".to_string(),
                    name: "Kimi For Coding".to_string(),
                },
                PiLoginProvider {
                    id: "future-provider".to_string(),
                    name: "future-provider".to_string(),
                },
            ]
        );
        assert!(
            state
                .db
                .get_all_providers(&crate::app_config::AppType::Pi)
                .expect("read saved providers")
                .is_empty(),
            "reading auth.json must not persist login projections"
        );
    }

    #[test]
    #[serial]
    fn invalid_auth_hides_only_login_projection() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let agent_dir = crate::pi_config::get_pi_agent_dir().expect("agent directory");
        fs::create_dir_all(&agent_dir).expect("create agent directory");
        fs::write(
            agent_dir.join("models.json"),
            r#"{"providers":{"custom":{"name":"Custom"}}}"#,
        )
        .expect("write models");
        fs::write(agent_dir.join("auth.json"), "{").expect("write invalid auth");

        let current = PiStateService::current(&state).expect("read explicit state");
        assert_eq!(current.enabled_provider_ids, vec!["custom"]);
        assert!(current.login_providers.is_empty());
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
