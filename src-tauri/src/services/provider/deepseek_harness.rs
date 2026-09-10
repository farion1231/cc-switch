use super::{ProviderService, SwitchResult};
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use crate::store::AppState;
use indexmap::IndexMap;
use serde_json::Value;

const APP: &str = "deepseek-harness";

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    if let Ok(native) = crate::deepseek_harness_config::read_native_state() {
        if let Err(error) = sync_native_locked(state, &native) {
            log::warn!("Failed to sync DeepSeek Harness providers: {error}");
        }
    }
    state.db.get_all_providers(APP)
}

pub(super) fn import_from_live(state: &AppState) -> Result<usize, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let native = crate::deepseek_harness_config::read_native_state()?;
    sync_native_locked(state, &native)
}

pub(super) fn add(
    state: &AppState,
    provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    if state.db.get_provider_by_id(&provider.id, APP)?.is_some() {
        return Err(AppError::InvalidInput(format!(
            "DeepSeek Harness provider '{}' already exists",
            provider.id
        )));
    }
    ProviderService::validate_provider_settings(
        &crate::app_config::AppType::DeepSeekHarness,
        &provider,
    )?;
    if add_to_live {
        write_native_provider(&provider)?;
    }
    state.db.save_provider(APP, &provider)?;
    if state.db.get_current_provider(APP)?.is_none() && add_to_live {
        let model = first_model_id(&provider.settings_config)?;
        crate::deepseek_harness_config::set_current_model(&provider.id, &model)?;
        state.db.set_current_provider(APP, &provider.id)?;
        crate::settings::set_current_provider(
            &crate::app_config::AppType::DeepSeekHarness,
            Some(&provider.id),
        )?;
    }
    Ok(true)
}

pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    provider: Provider,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let original_id = original_id.unwrap_or(&provider.id);
    if original_id != provider.id {
        return Err(AppError::InvalidInput(
            "DeepSeek Harness provider ids cannot be renamed".to_string(),
        ));
    }
    state
        .db
        .get_provider_by_id(original_id, APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("Provider '{original_id}' not found")))?;
    ProviderService::validate_provider_settings(
        &crate::app_config::AppType::DeepSeekHarness,
        &provider,
    )?;
    write_native_provider(&provider)?;
    state.db.save_provider(APP, &provider)?;
    Ok(true)
}

pub(super) fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let Some(provider) = state.db.get_provider_by_id(id, APP)? else {
        return Ok(());
    };
    remove_native_provider(&provider)?;
    state.db.delete_provider(APP, id)
}

pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let provider = state
        .db
        .get_provider_by_id(id, APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("Provider '{id}' not found")))?;
    write_native_provider(&provider)?;
    let native = crate::deepseek_harness_config::read_native_state()?;
    let model = if native.current_provider.as_deref() == Some(id) {
        native
            .current_model
            .filter(|model| provider_has_model(&provider.settings_config, model))
            .unwrap_or(first_model_id(&provider.settings_config)?)
    } else {
        first_model_id(&provider.settings_config)?
    };
    crate::deepseek_harness_config::set_current_model(id, &model)?;
    state.db.set_current_provider(APP, id)?;
    crate::settings::set_current_provider(&crate::app_config::AppType::DeepSeekHarness, Some(id))?;
    Ok(SwitchResult::default())
}

fn sync_native_locked(
    state: &AppState,
    native: &crate::deepseek_harness_config::NativeState,
) -> Result<usize, AppError> {
    let saved = state.db.get_all_providers(APP)?;
    let mut changed = 0;
    for (id, route) in &native.providers {
        let mut provider = saved.get(id).cloned().unwrap_or_else(|| {
            let mut provider =
                Provider::with_id(id.clone(), route.name.clone(), route.config.clone(), None);
            provider.category = Some(
                if route.source == crate::deepseek_harness_config::NativeProviderSource::DeepSeek {
                    "official"
                } else {
                    "custom"
                }
                .to_string(),
            );
            provider.icon = Some("deepseek".to_string());
            provider
        });
        let previous_name = provider.name.clone();
        let previous_config = provider.settings_config.clone();
        provider.name = route.name.clone();
        provider.settings_config = route.config.clone();
        provider
            .meta
            .get_or_insert_with(ProviderMeta::default)
            .provider_type = Some(
            match route.source {
                crate::deepseek_harness_config::NativeProviderSource::DeepSeek => "dsh_deepseek",
                crate::deepseek_harness_config::NativeProviderSource::PiAi => "dsh_pi_ai",
            }
            .to_string(),
        );
        if !saved.contains_key(id)
            || previous_name != provider.name
            || previous_config != provider.settings_config
        {
            state.db.save_provider(APP, &provider)?;
            changed += 1;
        }
    }
    if let Some(current) = native
        .current_provider
        .as_deref()
        .filter(|id| native.providers.contains_key(*id))
    {
        state.db.set_current_provider(APP, current)?;
        crate::settings::set_current_provider(
            &crate::app_config::AppType::DeepSeekHarness,
            Some(current),
        )?;
    }
    Ok(changed)
}

fn write_native_provider(provider: &Provider) -> Result<(), AppError> {
    if provider.id == crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID
        || provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref())
            == Some("dsh_deepseek")
    {
        let config = serde_json::from_value::<
            crate::deepseek_harness_config::DeepSeekHarnessProviderConfig,
        >(provider.settings_config.clone())
        .map_err(|error| AppError::Config(format!("Invalid DSH provider: {error}")))?;
        crate::deepseek_harness_config::set_provider(&provider.id, &config)
    } else {
        crate::deepseek_harness_config::set_pi_ai_provider(&provider.id, &provider.settings_config)
    }
}

fn remove_native_provider(provider: &Provider) -> Result<(), AppError> {
    if provider.id == crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID
        || provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref())
            == Some("dsh_deepseek")
    {
        crate::deepseek_harness_config::remove_provider()
    } else {
        crate::deepseek_harness_config::remove_pi_ai_provider(&provider.id)
    }
}

fn first_model_id(config: &Value) -> Result<String, AppError> {
    config
        .get("models")
        .and_then(Value::as_array)
        .and_then(|models| {
            models
                .iter()
                .find_map(|model| model.get("id").and_then(Value::as_str))
        })
        .filter(|id| !id.trim().is_empty())
        .map(ToOwned::to_owned)
        .ok_or_else(|| AppError::InvalidInput("DeepSeek Harness provider has no model".to_string()))
}

fn provider_has_model(config: &Value, model_id: &str) -> bool {
    config
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|model| model.get("id").and_then(Value::as_str) == Some(model_id))
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use serial_test::serial;
    use std::sync::Arc;

    struct DshHome {
        _dir: tempfile::TempDir,
        previous: Option<std::ffi::OsString>,
    }

    impl DshHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("DSH_HOME");
            std::env::set_var("DSH_HOME", dir.path());
            std::fs::write(
                dir.path().join("settings.yaml"),
                "llm-deepseek:\n  baseURL: https://api.deepseek.com\n  apiKeyEnv: DEEPSEEK_API_KEY\n  models:\n    - id: deepseek-v4-pro\nllm-pi-ai:\n  providers:\n    company:\n      displayName: Company\n      api: openai-completions\n      baseURL: https://gateway.example/v1\n      apiKeyEnv: COMPANY_API_KEY\n      models:\n        - id: glm-5.3\nagent-default-model:\n  provider: company\n  model: glm-5.3\n",
            )
            .unwrap();
            std::fs::write(
                dir.path().join(".credentials.yaml"),
                "version: 1\nrefs:\n  DEEPSEEK_API_KEY: deepseek-key\n  COMPANY_API_KEY: company-key\n",
            )
            .unwrap();
            Self {
                _dir: dir,
                previous,
            }
        }
    }

    impl Drop for DshHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("DSH_HOME", value),
                None => std::env::remove_var("DSH_HOME"),
            }
        }
    }

    fn state() -> AppState {
        AppState::new(Arc::new(Database::memory().unwrap()))
    }

    #[test]
    #[serial]
    fn imports_native_routes_and_current_provider() {
        let _home = DshHome::new();
        let state = state();
        assert_eq!(import_from_live(&state).unwrap(), 2);

        let providers = state.db.get_all_providers(APP).unwrap();
        assert_eq!(providers.len(), 2);
        assert_eq!(providers["company"].name, "Company");
        assert_eq!(
            providers["company"].settings_config["apiKey"],
            "company-key"
        );
        assert_eq!(
            state.db.get_current_provider(APP).unwrap().as_deref(),
            Some("company")
        );
    }

    #[test]
    #[serial]
    fn switching_provider_updates_native_default_model() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();

        enable(&state, crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID).unwrap();
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(
            native.current_provider.as_deref(),
            Some(crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID)
        );
        assert_eq!(native.current_model.as_deref(), Some("deepseek-v4-pro"));
    }

    #[test]
    #[serial]
    fn adds_and_deletes_custom_native_provider() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let mut provider = Provider::with_id(
            "new-route".to_string(),
            "New Route".to_string(),
            json!({
                "displayName": "New Route",
                "api": "openai-completions",
                "baseURL": "https://new.example/v1",
                "apiKeyEnv": "NEW_ROUTE_API_KEY",
                "apiKey": "new-secret",
                "models": [{"id": "model-a"}]
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("dsh_pi_ai".to_string()),
            ..Default::default()
        });

        add(&state, provider, true).unwrap();
        assert!(crate::deepseek_harness_config::read_native_state()
            .unwrap()
            .providers
            .contains_key("new-route"));
        delete(&state, "new-route").unwrap();
        assert!(!crate::deepseek_harness_config::read_native_state()
            .unwrap()
            .providers
            .contains_key("new-route"));
    }
}
