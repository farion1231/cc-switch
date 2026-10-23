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
    // The official route is a singleton keyed by id; a copy carrying its
    // provider_type would be routed to the same id-blind llm-deepseek
    // adapter, so a delete of the copy would wipe the real official route.
    if provider.id != crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID
        && provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.as_deref())
            == Some("dsh_deepseek")
    {
        return Err(AppError::InvalidInput(
            "Only the native official route may use the dsh_deepseek provider type".to_string(),
        ));
    }
    // Validate both catalog and saved default before any native, credential or
    // DB write, including when this is only being saved for later activation.
    let default_model = default_model_id(&provider)?;
    let should_select = if add_to_live {
        let native = crate::deepseek_harness_config::read_native_state()?;
        !native
            .current_provider
            .as_deref()
            .is_some_and(|id| native.providers.contains_key(id))
    } else {
        false
    };
    if add_to_live {
        crate::deepseek_harness_config::with_native_transaction(|| {
            write_native_provider(&provider)?;
            if should_select {
                crate::deepseek_harness_config::set_current_model(&provider.id, &default_model)?;
            }
            state.db.save_provider(APP, &provider)
        })?;
    } else {
        state.db.save_provider(APP, &provider)?;
    }
    if should_select {
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
    let native = crate::deepseek_harness_config::read_native_state()?;
    // DB-only providers (e.g. duplicates created with addToLive = false) must
    // stay out of the profile patch: an edit only updates the database row
    // until the user explicitly adds the route to the live configuration.
    let in_native = native.providers.contains_key(provider.id.as_str());
    // The edit form's default-model field only takes effect on the route that
    // is currently selected in DSH; applying it unconditionally would let an
    // edit of a background provider silently steal the native selection.
    let desired_model =
        if in_native && native.current_provider.as_deref() == Some(provider.id.as_str()) {
            provider
                .meta
                .as_ref()
                .and_then(|meta| meta.dsh_current_model.as_deref())
                .filter(|model| !model.trim().is_empty())
        } else {
            None
        };
    // Validate before any write so an unknown model cannot half-commit the
    // native file and the database row that the writes below already made.
    if let Some(model) = desired_model {
        if !provider_has_model(&provider.settings_config, model) {
            return Err(AppError::InvalidInput(format!(
                "Model '{model}' is not configured for DSH provider '{}'",
                provider.id
            )));
        }
    }
    let changes_model =
        desired_model.is_some_and(|model| native.current_model.as_deref() != Some(model));
    if in_native {
        crate::deepseek_harness_config::with_native_transaction(|| {
            write_native_provider(&provider)?;
            if changes_model {
                crate::deepseek_harness_config::set_current_model(
                    &provider.id,
                    desired_model.expect("changed model is present"),
                )?;
            }
            state.db.save_provider(APP, &provider)
        })?;
    } else {
        state.db.save_provider(APP, &provider)?;
    }
    if changes_model {
        let native = crate::deepseek_harness_config::read_native_state()?;
        sync_native_locked(state, &native)?;
    }
    Ok(true)
}

pub(super) fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let Some(provider) = state.db.get_provider_by_id(id, APP)? else {
        return Ok(());
    };
    crate::deepseek_harness_config::with_native_transaction(|| {
        remove_native_provider(&provider)?;
        crate::deepseek_harness_config::clear_current_model_if_provider(id)?;
        state.db.delete_provider(APP, id)
    })
}

pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let provider = state
        .db
        .get_provider_by_id(id, APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("Provider '{id}' not found")))?;
    // Reject invalid saved defaults/catalogs before write_native_provider can
    // change either the profile or credentials. Inspect the current selection
    // before writing too, so a read/selection error has no partial side effects.
    let default_model = default_model_id(&provider)?;
    let native = crate::deepseek_harness_config::read_native_state()?;
    let model = if native.current_provider.as_deref() == Some(id) {
        native
            .current_model
            .filter(|model| provider_has_model(&provider.settings_config, model))
            .unwrap_or(default_model)
    } else {
        default_model
    };
    crate::deepseek_harness_config::with_native_transaction(|| {
        write_native_provider(&provider)?;
        crate::deepseek_harness_config::set_current_model(id, &model)
    })?;
    let native = crate::deepseek_harness_config::read_native_state()?;
    sync_native_locked(state, &native)?;
    Ok(SwitchResult::default())
}

pub(super) fn set_current_model(
    state: &AppState,
    provider_id: &str,
    model_id: &str,
) -> Result<(), AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let native = crate::deepseek_harness_config::read_native_state()?;
    let provider = native
        .providers
        .get(provider_id)
        .ok_or_else(|| AppError::InvalidInput(format!("Provider '{provider_id}' not found")))?;
    if let Some(models) = provider.config.get("models").and_then(Value::as_array) {
        let known = models
            .iter()
            .any(|model| model_id_of(model) == Some(model_id));
        if !known {
            return Err(AppError::InvalidInput(format!(
                "Model '{model_id}' is not configured for DSH provider '{provider_id}'"
            )));
        }
    }
    crate::deepseek_harness_config::set_current_model(provider_id, model_id)?;
    let native = crate::deepseek_harness_config::read_native_state()?;
    sync_native_locked(state, &native)?;
    Ok(())
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
        let previous_source = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.provider_type.clone());
        let previous_current_model = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.dsh_current_model.clone());
        // Custom routes carry their display name in the patch entry, so a
        // rename lands in the native config on save and echoes back here.
        // The official route has no native display name: once the row exists,
        // keep the user's chosen name instead of resetting it to "DeepSeek".
        let is_official =
            route.source == crate::deepseek_harness_config::NativeProviderSource::DeepSeek;
        if !is_official || !saved.contains_key(id) {
            provider.name = route.name.clone();
        }
        provider.settings_config = route.config.clone();
        let meta = provider.meta.get_or_insert_with(ProviderMeta::default);
        meta.provider_type = Some(
            match route.source {
                crate::deepseek_harness_config::NativeProviderSource::DeepSeek => "dsh_deepseek",
                crate::deepseek_harness_config::NativeProviderSource::PiAi => "dsh_pi_ai",
            }
            .to_string(),
        );
        // The native pointer is authoritative for the current route only.
        // Background routes have no native default slot, so retain their saved
        // choice for a subsequent enable (unless its catalog no longer has it).
        meta.dsh_current_model = if native.current_provider.as_deref() == Some(id) {
            native.current_model.clone()
        } else {
            previous_current_model
                .as_ref()
                .filter(|model| provider_has_model(&provider.settings_config, model))
                .cloned()
        };
        if !saved.contains_key(id)
            || previous_name != provider.name
            || previous_config != provider.settings_config
            || previous_source != meta.provider_type
            || previous_current_model != meta.dsh_current_model
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

pub(super) fn remove_from_live(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let provider = state
        .db
        .get_provider_by_id(id, APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("Provider '{id}' not found")))?;
    crate::deepseek_harness_config::with_native_transaction(|| {
        remove_native_provider(&provider)?;
        crate::deepseek_harness_config::clear_current_model_if_provider(id)?;
        Ok(())
    })
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

pub(super) fn remove_native_provider(provider: &Provider) -> Result<(), AppError> {
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

/// A DSH model entry is either a bare id string or an `{id}` object; blank ids
/// never count as a usable model.
fn model_id_of(model: &Value) -> Option<&str> {
    match model {
        Value::String(id) => Some(id.as_str()),
        Value::Object(_) => model.get("id").and_then(Value::as_str),
        _ => None,
    }
    .filter(|id| !id.trim().is_empty())
}

fn first_model_id(config: &Value) -> Result<String, AppError> {
    config
        .get("models")
        .and_then(Value::as_array)
        .and_then(|models| models.iter().find_map(model_id_of))
        .map(ToOwned::to_owned)
        .ok_or_else(|| AppError::InvalidInput("DeepSeek Harness provider has no model".to_string()))
}

/// Resolve the form's optional default only after validating it against a
/// usable catalog. Callers must do this before starting any write path.
fn default_model_id(provider: &Provider) -> Result<String, AppError> {
    let first_model = first_model_id(&provider.settings_config)?;
    let requested = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.dsh_current_model.as_deref())
        .filter(|model| !model.trim().is_empty());
    if let Some(model) = requested {
        if !provider_has_model(&provider.settings_config, model) {
            return Err(AppError::InvalidInput(format!(
                "Model '{model}' is not configured for DSH provider '{}'",
                provider.id
            )));
        }
        Ok(model.to_string())
    } else {
        Ok(first_model)
    }
}

fn provider_has_model(config: &Value, model_id: &str) -> bool {
    config
        .get("models")
        .and_then(Value::as_array)
        .is_some_and(|models| {
            models
                .iter()
                .any(|model| model_id_of(model) == Some(model_id))
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
        previous_test_home: Option<std::ffi::OsString>,
    }

    impl DshHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let previous = std::env::var_os("DSH_HOME");
            std::env::set_var("DSH_HOME", dir.path());
            let previous_test_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            std::env::set_var("CC_SWITCH_TEST_HOME", dir.path());
            let profile_dir = dir.path().join("profiles/desktop");
            std::fs::create_dir_all(&profile_dir).unwrap();
            std::fs::write(
                profile_dir.join("cordis.patch.yml"),
                "- id: llm-deepseek\n  name: \"@deepseek-ai/dsh-llm-deepseek-api-key\"\n  config:\n    baseURL: https://api.deepseek.com\n    apiKeyEnv: DEEPSEEK_API_KEY\n    models:\n      - id: deepseek-v4-pro\n- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      company:\n        displayName: Company\n        api: openai-completions\n        baseURL: https://gateway.example/v1\n        apiKeyEnv: COMPANY_API_KEY\n        models:\n          - id: glm-5.3\n- id: agent-default-model\n  name: \"@deepseek-ai/dsh-agent-default-model\"\n  config:\n    provider: company\n    model: glm-5.3\n",
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
                previous_test_home,
            }
        }

        fn patch(&self) -> String {
            std::fs::read_to_string(self._dir.path().join("profiles/desktop/cordis.patch.yml"))
                .unwrap()
        }

        fn write_patch(&self, contents: &str) {
            std::fs::write(
                self._dir.path().join("profiles/desktop/cordis.patch.yml"),
                contents,
            )
            .unwrap();
        }
    }

    impl Drop for DshHome {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var("DSH_HOME", value),
                None => std::env::remove_var("DSH_HOME"),
            }
            match self.previous_test_home.take() {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn state() -> AppState {
        AppState::new(Arc::new(Database::memory().unwrap()))
    }

    fn two_model_provider(default: &str) -> Provider {
        let mut provider = Provider::with_id(
            "two-models".to_string(),
            "Two Models".to_string(),
            json!({
                "displayName": "Two Models",
                "api": "openai-completions",
                "baseURL": "https://two.example/v1",
                "apiKeyEnv": "TWO_MODELS_API_KEY",
                "apiKey": "test-only-key",
                "models": [{"id": "first"}, {"id": "second"}]
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            provider_type: Some("dsh_pi_ai".to_string()),
            dsh_current_model: Some(default.to_string()),
            ..Default::default()
        });
        provider
    }

    #[test]
    #[serial]
    fn add_first_provider_selects_second_model_default() {
        let home = DshHome::new();
        home.write_patch("[]\n");
        let state = state();
        add(&state, two_model_provider("second"), true).unwrap();
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(native.current_model.as_deref(), Some("second"));
    }

    #[test]
    #[serial]
    fn db_only_then_enable_uses_saved_second_model_default() {
        let home = DshHome::new();
        let state = state();
        let before = home.patch();
        add(&state, two_model_provider("second"), false).unwrap();
        assert_eq!(before, home.patch());
        enable(&state, "two-models").unwrap();
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(native.current_provider.as_deref(), Some("two-models"));
        assert_eq!(native.current_model.as_deref(), Some("second"));
    }

    #[test]
    #[serial]
    fn background_default_survives_sync_and_enable() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        add(&state, two_model_provider("second"), true).unwrap();
        list(&state).unwrap();
        enable(&state, "two-models").unwrap();
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(native.current_model.as_deref(), Some("second"));
        enable(&state, "company").unwrap();
        enable(&state, "two-models").unwrap();
        assert_eq!(
            crate::deepseek_harness_config::read_native_state()
                .unwrap()
                .current_model
                .as_deref(),
            Some("second")
        );
    }

    #[test]
    #[serial]
    fn enable_keeps_valid_current_native_model_over_saved_default() {
        let _home = DshHome::new();
        let state = state();
        add(&state, two_model_provider("second"), false).unwrap();
        write_native_provider(&two_model_provider("second")).unwrap();
        crate::deepseek_harness_config::set_current_model("two-models", "first").unwrap();
        enable(&state, "two-models").unwrap();
        assert_eq!(
            crate::deepseek_harness_config::read_native_state()
                .unwrap()
                .current_model
                .as_deref(),
            Some("first")
        );
    }

    #[test]
    #[serial]
    fn enable_stale_native_model_falls_back_to_saved_default() {
        let _home = DshHome::new();
        let state = state();
        add(&state, two_model_provider("second"), false).unwrap();
        write_native_provider(&two_model_provider("second")).unwrap();
        crate::deepseek_harness_config::set_current_model("two-models", "removed-model").unwrap();
        enable(&state, "two-models").unwrap();
        assert_eq!(
            crate::deepseek_harness_config::read_native_state()
                .unwrap()
                .current_model
                .as_deref(),
            Some("second")
        );
    }

    #[test]
    #[serial]
    fn enable_blank_saved_default_falls_back_to_first_model() {
        let _home = DshHome::new();
        let state = state();
        add(&state, two_model_provider("  "), false).unwrap();
        enable(&state, "two-models").unwrap();
        assert_eq!(
            crate::deepseek_harness_config::read_native_state()
                .unwrap()
                .current_model
                .as_deref(),
            Some("first")
        );
    }

    #[test]
    #[serial]
    fn default_model_home_override_rejects_add_and_enable_without_partial_writes() {
        let home = DshHome::new();
        home.write_patch("[]\n");
        std::fs::write(
            home._dir.path().join("cordis.patch.yml"),
            "- id: agent-default-model\n  config:\n    provider: locked\n    model: locked-model\n",
        )
        .unwrap();
        let state = state();
        let patch = home.patch();
        let credentials = std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap();
        assert!(add(&state, two_model_provider("second"), true).is_err());
        assert_eq!(home.patch(), patch);
        assert_eq!(
            std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap(),
            credentials
        );
        assert!(state.db.get_all_providers(APP).unwrap().is_empty());

        add(&state, two_model_provider("second"), false).unwrap();
        let before = serde_json::to_value(state.db.get_all_providers(APP).unwrap()).unwrap();
        assert!(enable(&state, "two-models").is_err());
        assert_eq!(home.patch(), patch);
        assert_eq!(
            std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap(),
            credentials
        );
        assert_eq!(
            serde_json::to_value(state.db.get_all_providers(APP).unwrap()).unwrap(),
            before
        );
        assert!(state.db.get_current_provider(APP).unwrap().is_none());
    }

    #[test]
    #[serial]
    fn home_default_conflict_rolls_back_update_delete_and_remove() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        std::fs::write(
            home._dir.path().join("cordis.patch.yml"),
            "- id: agent-default-model\n  disabled: false\n  config:\n    provider: company\n    model: glm-5.3\n",
        )
        .unwrap();
        let patch = home.patch();
        let credentials = std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap();
        let before = serde_json::to_value(state.db.get_all_providers(APP).unwrap()).unwrap();
        let mut provider = state
            .db
            .get_provider_by_id("company", APP)
            .unwrap()
            .unwrap();
        provider.settings_config["models"] = json!([{"id": "glm-5.3"}, {"id": "second"}]);
        provider.settings_config["apiKey"] = json!("replacement-test-key");
        provider.meta.as_mut().unwrap().dsh_current_model = Some("second".into());
        assert!(update(&state, None, provider).is_err());
        assert_eq!(home.patch(), patch);
        assert_eq!(
            std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap(),
            credentials
        );
        assert_eq!(
            serde_json::to_value(state.db.get_all_providers(APP).unwrap()).unwrap(),
            before
        );
        for remove_only in [false, true] {
            let result = if remove_only {
                remove_from_live(&state, "company")
            } else {
                delete(&state, "company")
            };
            assert!(result.is_err());
            assert_eq!(home.patch(), patch);
            assert_eq!(
                std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap(),
                credentials
            );
            assert_eq!(
                serde_json::to_value(state.db.get_all_providers(APP).unwrap()).unwrap(),
                before
            );
        }
    }

    #[test]
    #[serial]
    fn add_unknown_default_has_no_native_db_or_credentials_writes() {
        let home = DshHome::new();
        let state = state();
        for live in [false, true] {
            let patch = home.patch();
            let credentials = std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap();
            assert!(add(&state, two_model_provider("unknown"), live).is_err());
            assert_eq!(home.patch(), patch);
            assert_eq!(
                std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap(),
                credentials
            );
            assert!(state.db.get_all_providers(APP).unwrap().is_empty());
            assert!(state.db.get_current_provider(APP).unwrap().is_none());
        }
    }

    #[test]
    #[serial]
    fn enable_invalid_catalog_or_default_has_no_partial_writes() {
        let home = DshHome::new();
        let state = state();
        for no_models in [false, true] {
            let mut provider = two_model_provider("unknown");
            if no_models {
                provider.settings_config["models"] = json!([]);
                provider.meta.as_mut().unwrap().dsh_current_model = None;
            }
            state.db.save_provider(APP, &provider).unwrap();
            let before = serde_json::to_value(state.db.get_all_providers(APP).unwrap()).unwrap();
            let patch = home.patch();
            let credentials = std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap();
            assert!(enable(&state, &provider.id).is_err());
            assert_eq!(home.patch(), patch);
            assert_eq!(
                std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap(),
                credentials
            );
            assert_eq!(
                serde_json::to_value(state.db.get_all_providers(APP).unwrap()).unwrap(),
                before
            );
            assert!(state.db.get_current_provider(APP).unwrap().is_none());
        }
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
        assert_eq!(
            providers["company"]
                .meta
                .as_ref()
                .and_then(|meta| meta.dsh_current_model.as_deref()),
            Some("glm-5.3")
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

    #[test]
    #[serial]
    fn set_current_model_validates_membership_and_updates_state() {
        let home = DshHome::new();
        home.write_patch(
            "- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      k3:\n        baseURL: https://k3.example.com\n        models:\n          - id: k3\n          - id: k3-turbo\n",
        );
        let state = state();

        set_current_model(&state, "k3", "k3-turbo").unwrap();
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(native.current_provider.as_deref(), Some("k3"));
        assert_eq!(native.current_model.as_deref(), Some("k3-turbo"));
        assert_eq!(
            state.db.get_current_provider(APP).unwrap().as_deref(),
            Some("k3")
        );

        assert!(set_current_model(&state, "k3", "no-such-model").is_err());
        assert!(set_current_model(&state, "ghost", "k3").is_err());
    }

    #[test]
    #[serial]
    fn remove_from_live_only_clears_the_targeted_route() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let mut provider = Provider::with_id(
            "k3".to_string(),
            "K3".to_string(),
            json!({
                "displayName": "K3",
                "api": "openai-completions",
                "baseURL": "https://k3.example.com",
                "apiKeyEnv": "K3_API_KEY",
                "apiKey": "k3-secret",
                "models": [{"id": "k3-model"}]
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("dsh_pi_ai".to_string()),
            ..Default::default()
        });
        add(&state, provider, true).unwrap();

        remove_from_live(&state, "k3").unwrap();

        let patch = home.patch();
        assert!(patch.contains("llm-deepseek"));
        assert!(patch.contains("company"));
        assert!(!patch.contains("k3"));
    }

    #[test]
    #[serial]
    fn remove_from_live_official_route_keeps_pi_ai_providers() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();

        remove_from_live(&state, crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID).unwrap();

        let patch = home.patch();
        // A profile deletion may retain a $remove tombstone to suppress base
        // bundle defaults; assert effective state rather than patch spelling.
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert!(!native
            .providers
            .contains_key(crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID));
        assert!(patch.contains("llm-pi-ai"));
        assert!(patch.contains("company"));
        // The removed route was not the current one: the default-model pointer
        // must survive untouched.
        assert!(patch.contains("agent-default-model"));
    }

    #[test]
    #[serial]
    fn remove_keeps_credentials_still_referenced_by_sibling_routes() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        // Make both pi-ai routes share one credential ref.
        home.write_patch(
            "- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      route-a:\n        displayName: A\n        baseURL: https://a.example/v1\n        apiKeyEnv: COMPANY_API_KEY\n        models:\n          - id: m-a\n      route-b:\n        displayName: B\n        baseURL: https://b.example/v1\n        apiKeyEnv: COMPANY_API_KEY\n        models:\n          - id: m-b\n",
        );
        import_from_live(&state).unwrap();

        delete(&state, "route-a").unwrap();
        let credentials =
            std::fs::read_to_string(home._dir.path().join(".credentials.yaml")).unwrap();
        assert!(
            credentials.contains("COMPANY_API_KEY"),
            "shared ref must survive while route-b still uses it"
        );

        delete(&state, "route-b").unwrap();
        let credentials =
            std::fs::read_to_string(home._dir.path().join(".credentials.yaml")).unwrap();
        assert!(
            !credentials.contains("COMPANY_API_KEY"),
            "unreferenced credentials are cleaned up"
        );
    }

    #[test]
    #[serial]
    fn remove_official_route_keeps_shared_deepseek_credential() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        // The custom route shares DEEPSEEK_API_KEY with the official route.
        home.write_patch(
            "- id: llm-deepseek\n  name: \"@deepseek-ai/dsh-llm-deepseek-api-key\"\n  config:\n    baseURL: https://api.deepseek.com\n    apiKeyEnv: DEEPSEEK_API_KEY\n    models:\n      - id: deepseek-v4-pro\n- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      company:\n        displayName: Company\n        baseURL: https://gateway.example/v1\n        apiKeyEnv: DEEPSEEK_API_KEY\n        models:\n          - id: glm-5.3\n",
        );
        import_from_live(&state).unwrap();

        remove_from_live(&state, crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID).unwrap();

        let credentials =
            std::fs::read_to_string(home._dir.path().join(".credentials.yaml")).unwrap();
        assert!(
            credentials.contains("DEEPSEEK_API_KEY"),
            "the official route must not clear a shared credential"
        );
    }

    #[test]
    #[serial]
    fn update_keeps_db_only_providers_out_of_native_config() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let mut copy = Provider::with_id(
            "company-copy".to_string(),
            "Company Copy".to_string(),
            json!({
                "displayName": "Company Copy",
                "api": "openai-completions",
                "baseURL": "https://copy.example/v1",
                "apiKeyEnv": "COMPANY_API_KEY",
                "apiKey": "secret",
                "models": [{"id": "m-1"}]
            }),
            None,
        );
        copy.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("dsh_pi_ai".to_string()),
            ..Default::default()
        });
        add(&state, copy, false).unwrap();
        let patch_before = home.patch();
        let credentials_before = std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap();

        let mut edited = state
            .db
            .get_provider_by_id("company-copy", APP)
            .unwrap()
            .unwrap();
        edited.name = "Renamed Copy".to_string();
        update(&state, None, edited).unwrap();
        crate::services::provider::live::sync_additive_app_to_live(
            &state,
            &crate::app_config::AppType::DeepSeekHarness,
        )
        .unwrap();

        assert_eq!(
            patch_before,
            home.patch(),
            "editing a DB-only provider must not touch the native file"
        );
        assert_eq!(
            credentials_before,
            std::fs::read(home._dir.path().join(".credentials.yaml")).unwrap(),
            "editing or syncing DB-only providers must not change credential refs"
        );
        assert!(!crate::deepseek_harness_config::read_native_state()
            .unwrap()
            .providers
            .contains_key("company-copy"));
    }

    #[test]
    #[serial]
    fn deleting_the_current_provider_clears_the_native_default_model() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();

        delete(&state, "company").unwrap();

        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert!(native.current_provider.is_none());
        assert!(native.current_model.is_none());
    }

    #[test]
    #[serial]
    fn remove_from_live_of_the_current_provider_clears_the_default_model() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        enable(&state, "company").unwrap();

        remove_from_live(&state, "company").unwrap();

        let patch = home.patch();
        assert!(!patch.contains("company"));
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert!(native.current_provider.is_none());
        assert!(native.current_model.is_none());
    }

    #[test]
    #[serial]
    fn update_applies_the_default_model_for_the_current_provider() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let mut provider = state
            .db
            .get_provider_by_id("company", APP)
            .unwrap()
            .unwrap();
        provider.meta.as_mut().unwrap().dsh_current_model = Some("new-model".to_string());

        // The model is not in the catalog yet: the edit must be rejected.
        assert!(update(&state, None, provider.clone()).is_err());

        provider.settings_config["models"] = json!([{ "id": "glm-5.3" }, { "id": "new-model" }]);
        update(&state, None, provider).unwrap();

        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(native.current_provider.as_deref(), Some("company"));
        assert_eq!(native.current_model.as_deref(), Some("new-model"));
    }

    #[test]
    #[serial]
    fn update_ignores_the_default_model_for_background_providers() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let mut official = state
            .db
            .get_provider_by_id(crate::deepseek_harness_config::OFFICIAL_PROVIDER_ID, APP)
            .unwrap()
            .unwrap();
        official.meta.as_mut().unwrap().dsh_current_model = Some("deepseek-v4-pro".to_string());

        update(&state, None, official).unwrap();

        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(native.current_provider.as_deref(), Some("company"));
        assert_eq!(native.current_model.as_deref(), Some("glm-5.3"));
    }

    #[test]
    #[serial]
    fn update_rejects_unknown_default_model_without_writing() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let mut provider = state
            .db
            .get_provider_by_id("company", APP)
            .unwrap()
            .unwrap();
        provider.meta.as_mut().unwrap().dsh_current_model = Some("no-such-model".to_string());

        assert!(update(&state, None, provider).is_err());

        // The rejected edit must not reach the database row: the native file
        // may rewrite byte-identically, but the meta must keep the old model.
        let saved = state
            .db
            .get_provider_by_id("company", APP)
            .unwrap()
            .unwrap();
        assert_ne!(
            saved.meta.as_ref().unwrap().dsh_current_model.as_deref(),
            Some("no-such-model")
        );
    }

    #[test]
    #[serial]
    fn add_rejects_dsh_deepseek_meta_on_a_copy() {
        let home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let before = home.patch();

        let mut provider = Provider::with_id(
            "official-copy".to_string(),
            "DeepSeek copy".to_string(),
            json!({
                "displayName": "DeepSeek copy",
                "baseURL": "https://api.deepseek.com",
                "apiKey": "secret",
                "models": [{ "id": "deepseek-v4-pro" }]
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("dsh_deepseek".to_string()),
            ..Default::default()
        });

        assert!(add(&state, provider, true).is_err());

        assert_eq!(before, home.patch());
        assert!(state
            .db
            .get_provider_by_id("official-copy", APP)
            .unwrap()
            .is_none());
    }

    #[test]
    #[serial]
    fn add_rejects_model_less_providers_without_partial_writes() {
        let _home = DshHome::new();
        let state = state();
        import_from_live(&state).unwrap();
        let mut provider = Provider::with_id(
            "no-models".to_string(),
            "No Models".to_string(),
            json!({
                "displayName": "No Models",
                "api": "openai-completions",
                "baseURL": "https://x.example/v1",
                "apiKeyEnv": "NO_MODELS_API_KEY",
                "apiKey": "secret",
                "models": []
            }),
            None,
        );
        provider.meta = Some(crate::provider::ProviderMeta {
            provider_type: Some("dsh_pi_ai".to_string()),
            ..Default::default()
        });

        assert!(add(&state, provider, true).is_err());
        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert!(!native.providers.contains_key("no-models"));
        assert!(state
            .db
            .get_provider_by_id("no-models", APP)
            .unwrap()
            .is_none());
    }

    #[test]
    #[serial]
    fn enable_supports_string_form_model_catalogs_and_skips_blank_ids() {
        let home = DshHome::new();
        home.write_patch(
            "- id: llm-pi-ai\n  name: \"@deepseek-ai/dsh-llm-pi-ai\"\n  config:\n    providers:\n      strings:\n        baseURL: https://s.example\n        models:\n          - \"\"\n          - str-model\n",
        );
        let state = state();
        import_from_live(&state).unwrap();

        enable(&state, "strings").unwrap();

        let native = crate::deepseek_harness_config::read_native_state().unwrap();
        assert_eq!(native.current_provider.as_deref(), Some("strings"));
        assert_eq!(native.current_model.as_deref(), Some("str-model"));
    }
}
