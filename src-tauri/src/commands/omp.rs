use crate::error::AppError;
use crate::services::pi_state::PiCurrentState;
use crate::store::AppState;
use tauri::State;

/// Explicit providers in models.yml, plus the provider part of `modelRoles.default` (which may
/// name a built-in provider with no explicit node). Same shape as Pi's state.
fn current_state(state: &AppState) -> Result<PiCurrentState, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    let enabled_provider_ids = crate::omp_config::get_providers()?.into_keys().collect();
    let default_provider_id =
        crate::omp_config::read_default_provider_id().unwrap_or_else(|error| {
            log::warn!("Failed to read Oh My Pi default model: {error}");
            None
        });
    Ok(PiCurrentState {
        enabled_provider_ids,
        default_provider_id,
    })
}

#[tauri::command]
pub(crate) fn get_omp_current_state(state: State<'_, AppState>) -> Result<PiCurrentState, String> {
    current_state(state.inner()).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::omp_config::test_support::TestAgentDir;
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;

    fn write(models: &str, settings: &str) {
        let dir = crate::omp_config::get_omp_agent_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("models.yml"), models).unwrap();
        fs::write(dir.join("config.yml"), settings).unwrap();
    }

    #[test]
    #[serial]
    fn state_exposes_explicit_providers_and_default() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        write(
            "providers:\n  managed:\n    baseUrl: https://api.example.com/v1\n    api: openai-completions\n    models:\n      - id: model-a\n  native:\n    baseUrl: https://native.example\n",
            "modelRoles:\n  default: managed/model-a\n",
        );
        let current = current_state(&state).unwrap();
        assert_eq!(current.enabled_provider_ids, vec!["managed", "native"]);
        assert_eq!(current.default_provider_id.as_deref(), Some("managed"));
    }

    #[test]
    #[serial]
    fn built_in_default_without_explicit_node_is_tolerated() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        write("providers: {}\n", "modelRoles:\n  default: openai/gpt-4o\n");
        let current = current_state(&state).unwrap();
        assert!(current.enabled_provider_ids.is_empty());
        assert_eq!(current.default_provider_id.as_deref(), Some("openai"));
    }
}
