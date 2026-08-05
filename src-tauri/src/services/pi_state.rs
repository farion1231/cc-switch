//! Read-only view of the Pi-owned provider and model selection.

use crate::error::AppError;
use crate::pi_config::{
    is_pi_builtin_provider_key, is_pi_owned_provider, read_pi_native_defaults,
    read_pi_native_providers,
};
use crate::store::AppState;
use serde::Serialize;

const PI_APP: &str = "pi";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PiCurrentOwnership {
    Managed,
    PiNative,
    External,
    Unconfigured,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PiCurrentState {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub managed_provider_id: Option<String>,
    pub ownership: PiCurrentOwnership,
    pub enabled_provider_ids: Vec<String>,
    pub drifted_provider_ids: Vec<String>,
}

pub(crate) struct PiStateService;

impl PiStateService {
    pub(crate) fn current(state: &AppState) -> Result<PiCurrentState, AppError> {
        let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(PI_APP));
        let defaults = read_pi_native_defaults()?;
        let native = read_pi_native_providers()?;
        let saved = state.db.get_all_providers(PI_APP)?;
        let enabled_provider_ids = saved
            .iter()
            .filter(|(id, _)| native.contains_key(id.as_str()))
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        let drifted_provider_ids = saved
            .iter()
            .filter(|(id, provider)| {
                native
                    .get(id.as_str())
                    .is_some_and(|config| *config != provider.settings_config)
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();

        let Some(provider_key) = defaults.default_provider else {
            return Ok(PiCurrentState {
                provider_key: None,
                model_id: defaults.default_model,
                managed_provider_id: None,
                ownership: PiCurrentOwnership::Unconfigured,
                enabled_provider_ids,
                drifted_provider_ids,
            });
        };

        let managed = saved.get(&provider_key).is_some_and(|provider| {
            native
                .get(&provider_key)
                .is_some_and(|config| *config == provider.settings_config)
        });
        let ownership = if managed {
            PiCurrentOwnership::Managed
        } else if is_pi_builtin_provider_key(&provider_key)
            || native
                .get(&provider_key)
                .is_some_and(|config| is_pi_owned_provider(&provider_key, config))
        {
            PiCurrentOwnership::PiNative
        } else {
            PiCurrentOwnership::External
        };

        Ok(PiCurrentState {
            provider_key: Some(provider_key.clone()),
            model_id: defaults.default_model,
            managed_provider_id: managed.then_some(provider_key),
            ownership,
            enabled_provider_ids,
            drifted_provider_ids,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::pi_config::test_support::TestAgentDir;
    use crate::provider::Provider;
    use crate::services::ProviderService;
    use serde_json::json;
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;

    #[test]
    #[serial]
    fn state_is_derived_from_settings_models_and_saved_provider() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ));
        let config = json!({
            "name": "Managed",
            "baseUrl": "https://api.example.com/v1",
            "apiKey": "secret",
            "api": "openai-completions",
            "models": [{ "id": "model-a" }]
        });
        ProviderService::add(
            &state,
            AppType::Pi,
            Provider {
                id: "cc-switch-managed".to_string(),
                name: "Managed".to_string(),
                settings_config: config.clone(),
                website_url: None,
                category: Some("custom".to_string()),
                created_at: None,
                sort_index: None,
                notes: None,
                meta: None,
                icon: None,
                icon_color: None,
                in_failover_queue: false,
            },
            true,
        )
        .expect("add provider");
        let settings_path = crate::pi_config::get_pi_settings_path().expect("settings path");
        fs::create_dir_all(settings_path.parent().expect("settings directory"))
            .expect("create settings directory");
        fs::write(
            settings_path,
            r#"{"defaultProvider":"cc-switch-managed","defaultModel":"model-a"}"#,
        )
        .expect("write settings");

        let current = PiStateService::current(&state).expect("read state");
        assert_eq!(current.ownership, PiCurrentOwnership::Managed);
        assert_eq!(
            current.enabled_provider_ids,
            vec!["cc-switch-managed".to_string()]
        );

        let mut drifted = config.clone();
        drifted["name"] = json!("External edit");
        crate::pi_config::replace_pi_provider("cc-switch-managed", &config, &drifted)
            .expect("edit models.json");
        let current = PiStateService::current(&state).expect("read drifted state");
        assert_eq!(current.ownership, PiCurrentOwnership::External);
        assert_eq!(
            current.enabled_provider_ids,
            vec!["cc-switch-managed".to_string()]
        );
        assert_eq!(
            current.drifted_provider_ids,
            vec!["cc-switch-managed".to_string()]
        );
    }
}
