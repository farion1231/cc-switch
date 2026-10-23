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
    /// Credential references (`apiKeyEnv`) the native routes currently use;
    /// new custom routes pick a reference that avoids every one of them.
    pub credential_refs: Vec<String>,
}

pub(crate) struct DshStateService;

impl DshStateService {
    pub(crate) fn current(state: &AppState) -> Result<DshCurrentState, AppError> {
        let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(DSH_APP));
        let native = crate::deepseek_harness_config::read_native_state()?;
        let mut credential_refs: Vec<String> = native
            .providers
            .values()
            .filter_map(|provider| {
                provider
                    .config
                    .get("apiKeyEnv")
                    .and_then(serde_json::Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .map(ToOwned::to_owned)
            })
            .collect();
        // Reserve orphaned/disabled references too: they can still belong to
        // another profile and must not be overwritten by a new route.
        credential_refs.extend(crate::deepseek_harness_config::credential_reference_names()?);
        credential_refs.sort();
        credential_refs.dedup();
        Ok(DshCurrentState {
            provider_ids: native.providers.keys().cloned().collect(),
            current_provider_id: native.current_provider,
            current_model: native.current_model,
            credential_refs,
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
            let profile = home.join("profiles/desktop");
            std::fs::create_dir_all(&profile).unwrap();
            std::fs::write(
                profile.join("cordis.patch.yml"),
                "- id: llm-deepseek\n  name: \"@deepseek-ai/dsh-llm-deepseek-api-key\"\n  config:\n    baseURL: https://api.deepseek.com\n    apiKeyEnv: DEEPSEEK_API_KEY\n- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      k3:\n        baseURL: https://k3.example.com\n        apiKeyEnv: K3_API_KEY\n- id: agent-default-model\n  name: \"@deepseek-ai/dsh-agent-default-model\"\n  config:\n    provider: k3\n    model: k3\n",
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
            assert!(current
                .credential_refs
                .contains(&"DEEPSEEK_API_KEY".to_string()));
            assert!(current.credential_refs.contains(&"K3_API_KEY".to_string()));
        });
    }

    #[test]
    #[serial]
    fn reserves_credentials_not_referenced_by_active_routes() {
        with_temp_home(|home| {
            std::fs::write(
                home.join(".credentials.yaml"),
                "version: 1\nrefs:\n  ORPHAN_API_KEY: test-only\nrecords: {}\n",
            )
            .unwrap();
            let state = AppState::new(Arc::new(crate::database::Database::memory().unwrap()));
            let current = DshStateService::current(&state).unwrap();
            assert_eq!(current.credential_refs, vec!["ORPHAN_API_KEY"]);
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
            assert!(current.credential_refs.is_empty());
        });
    }
}
