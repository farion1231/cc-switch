use super::{ProviderService, SwitchResult};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use crate::store::AppState;

pub(super) fn add(
    state: &AppState,
    mut provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let app_type = AppType::Pi;
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));
    strip_unsupported_pi_metadata(&mut provider);
    ProviderService::validate_provider_settings(&app_type, &provider)?;
    ProviderService::normalize_usage_script_credential_overrides(&app_type, &mut provider);

    if state
        .db
        .get_provider_by_id(&provider.id, app_type.as_str())?
        .is_some()
    {
        return Err(AppError::Conflict(format!(
            "Pi provider '{}' already exists",
            provider.id
        )));
    }

    let native_inserted = if add_to_live {
        // Membership changes fail closed when Pi's selection file is
        // unreadable. Database-only copies remain available because they do
        // not touch Pi-owned state.
        crate::pi_config::read_pi_native_defaults()?;
        crate::pi_config::insert_pi_provider(&provider.id, &provider.settings_config)?
    } else {
        false
    };

    if let Err(error) = state.db.save_provider(app_type.as_str(), &provider) {
        if native_inserted {
            if let Err(rollback) =
                crate::pi_config::remove_pi_provider(&provider.id, &provider.settings_config)
            {
                return Err(AppError::Config(format!(
                    "failed to save Pi provider: {error}; native rollback failed: {rollback}"
                )));
            }
        }
        return Err(error);
    }
    Ok(true)
}

pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    mut provider: Provider,
) -> Result<bool, AppError> {
    let app_type = AppType::Pi;
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));
    let original_id = original_id.unwrap_or(&provider.id).to_string();
    if original_id != provider.id {
        return Err(AppError::InvalidInput(
            "Pi provider keys cannot be renamed".to_string(),
        ));
    }

    let existing = state
        .db
        .get_provider_by_id(&original_id, app_type.as_str())?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{original_id}' not found")))?;
    strip_unsupported_pi_metadata(&mut provider);
    ProviderService::validate_provider_settings(&app_type, &provider)?;
    ProviderService::normalize_usage_script_credential_overrides(&app_type, &mut provider);

    let native = crate::pi_config::read_pi_native_provider(&original_id)?;
    let live_enabled = match native {
        None => false,
        Some(config) if config == existing.settings_config => true,
        Some(_) => {
            return Err(AppError::Conflict(format!(
                "Pi provider '{original_id}' changed outside CC Switch"
            )))
        }
    };
    let defaults = crate::pi_config::read_pi_native_defaults()?;
    if defaults.default_provider.as_deref() == Some(original_id.as_str()) {
        if let Some(model_id) = defaults.default_model.as_deref() {
            if crate::pi_config::provider_contains_model(&existing.settings_config, model_id)
                && !crate::pi_config::provider_contains_model(&provider.settings_config, model_id)
            {
                return Err(AppError::InvalidInput(format!(
                    "Pi is currently using model '{model_id}'; choose another model in Pi before removing it"
                )));
            }
        }
    }

    if live_enabled {
        crate::pi_config::replace_pi_provider(
            &original_id,
            &existing.settings_config,
            &provider.settings_config,
        )?;
    }
    if let Err(error) = state.db.save_provider(app_type.as_str(), &provider) {
        if live_enabled {
            if let Err(rollback) = crate::pi_config::replace_pi_provider(
                &original_id,
                &provider.settings_config,
                &existing.settings_config,
            ) {
                return Err(AppError::Config(format!(
                    "failed to save Pi provider: {error}; native rollback failed: {rollback}"
                )));
            }
        }
        return Err(error);
    }
    Ok(true)
}

pub(super) fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let app_type = AppType::Pi;
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));
    ensure_not_current(id)?;
    let Some(provider) = state.db.get_provider_by_id(id, app_type.as_str())? else {
        return Ok(());
    };
    let removed = crate::pi_config::remove_pi_provider(id, &provider.settings_config)?;

    if let Err(error) = state.db.delete_provider(app_type.as_str(), id) {
        if removed {
            if let Err(rollback) =
                crate::pi_config::restore_pi_provider_if_missing(id, &provider.settings_config)
            {
                return Err(AppError::Config(format!(
                    "failed to delete Pi provider: {error}; native rollback failed: {rollback}"
                )));
            }
        }
        return Err(error);
    }
    Ok(())
}

pub(super) fn remove(state: &AppState, id: &str) -> Result<(), AppError> {
    let app_type = AppType::Pi;
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));
    ensure_not_current(id)?;
    let provider = state
        .db
        .get_provider_by_id(id, app_type.as_str())?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{id}' not found")))?;
    crate::pi_config::remove_pi_provider(id, &provider.settings_config)?;
    Ok(())
}

pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let app_type = AppType::Pi;
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));
    let provider = state
        .db
        .get_provider_by_id(id, app_type.as_str())?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{id}' not found")))?;
    ProviderService::validate_provider_settings(&app_type, &provider)?;

    crate::pi_config::read_pi_native_defaults()?;
    crate::pi_config::insert_pi_provider(id, &provider.settings_config)?;
    Ok(SwitchResult::default())
}

fn ensure_not_current(provider_id: &str) -> Result<(), AppError> {
    if crate::pi_config::read_pi_native_defaults()?
        .default_provider
        .as_deref()
        == Some(provider_id)
    {
        return Err(AppError::InvalidInput(
            "This provider is currently selected in Pi; switch with Pi /model before removing it"
                .to_string(),
        ));
    }
    Ok(())
}

fn strip_unsupported_pi_metadata(provider: &mut Provider) {
    provider.in_failover_queue = false;
    let Some(meta) = provider.meta.take() else {
        return;
    };
    provider.meta = Some(ProviderMeta {
        usage_script: meta.usage_script,
        is_partner: meta.is_partner,
        partner_promotion_key: meta.partner_promotion_key,
        cost_multiplier: meta.cost_multiplier,
        pricing_model_source: meta.pricing_model_source,
        limit_daily_usd: meta.limit_daily_usd,
        limit_monthly_usd: meta.limit_monthly_usd,
        ..ProviderMeta::default()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use crate::pi_config::test_support::TestAgentDir;
    use crate::provider::ProviderMeta;
    use serde_json::json;
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;

    fn state() -> AppState {
        AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ))
    }

    fn input(model_id: &str) -> Provider {
        Provider {
            id: "cc-switch-test".to_string(),
            name: "Test provider".to_string(),
            settings_config: json!({
                "name": "Test provider",
                "baseUrl": "https://api.example.com/v1",
                "apiKey": "secret",
                "api": "openai-completions",
                "models": [{ "id": model_id }]
            }),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                common_config_enabled: Some(true),
                endpoint_auto_select: Some(true),
                live_config_managed: Some(false),
                api_format: Some("openai_chat".to_string()),
                custom_user_agent: Some("legacy-route-agent".to_string()),
                is_partner: Some(true),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: false,
        }
    }

    #[test]
    #[serial]
    fn membership_is_derived_only_from_models_json() {
        let _agent = TestAgentDir::new();
        let state = state();

        ProviderService::add(&state, AppType::Pi, input("model-a"), false)
            .expect("save disabled provider");
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());

        let saved = state
            .db
            .get_provider_by_id("cc-switch-test", "pi")
            .unwrap()
            .unwrap();
        let meta = saved.meta.unwrap_or_default();
        assert_eq!(meta.common_config_enabled, None);
        assert_eq!(meta.live_config_managed, None);
        assert_eq!(meta.endpoint_auto_select, None);
        assert_eq!(meta.api_format, None);
        assert_eq!(meta.custom_user_agent, None);
        assert_eq!(meta.is_partner, Some(true));

        ProviderService::switch(&state, AppType::Pi, "cc-switch-test").expect("enable provider");
        assert!(crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());

        ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
            .expect("remove provider");
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
        assert!(state
            .db
            .get_provider_by_id("cc-switch-test", "pi")
            .unwrap()
            .is_some());
    }

    #[test]
    #[serial]
    fn current_provider_and_model_are_pi_owned() {
        let _agent = TestAgentDir::new();
        let state = state();
        ProviderService::add(&state, AppType::Pi, input("model-a"), true).expect("add provider");
        let settings_path = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(
            settings_path,
            r#"{"defaultProvider":"cc-switch-test","defaultModel":"model-a"}"#,
        )
        .unwrap();

        assert!(
            ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
                .is_err()
        );
        assert!(ProviderService::delete(&state, AppType::Pi, "cc-switch-test").is_err());
        assert!(ProviderService::update(
            &state,
            AppType::Pi,
            Some("cc-switch-test"),
            input("model-b"),
        )
        .is_err());
        assert!(crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
    }

    #[test]
    #[serial]
    fn failed_duplicate_create_rolls_back_native_insertion() {
        let _agent = TestAgentDir::new();
        let state = state();
        ProviderService::add(&state, AppType::Pi, input("model-a"), false)
            .expect("save DB-only provider");

        assert!(ProviderService::add(&state, AppType::Pi, input("model-a"), true).is_err());
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
    }

    #[test]
    #[serial]
    fn external_native_edit_blocks_managed_mutations() {
        let _agent = TestAgentDir::new();
        let state = state();
        ProviderService::add(&state, AppType::Pi, input("model-a"), true).expect("add provider");
        let saved = state
            .db
            .get_provider_by_id("cc-switch-test", "pi")
            .unwrap()
            .unwrap();
        let mut external = saved.settings_config.clone();
        external["name"] = json!("External edit");
        crate::pi_config::replace_pi_provider("cc-switch-test", &saved.settings_config, &external)
            .expect("edit native provider");

        assert!(ProviderService::update(
            &state,
            AppType::Pi,
            Some("cc-switch-test"),
            input("model-b"),
        )
        .is_err());
        assert!(
            ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
                .is_err()
        );
        assert!(ProviderService::delete(&state, AppType::Pi, "cc-switch-test").is_err());
        assert_eq!(
            state
                .db
                .get_provider_by_id("cc-switch-test", "pi")
                .unwrap()
                .unwrap()
                .settings_config,
            saved.settings_config
        );
    }

    #[test]
    #[serial]
    fn unreadable_settings_block_membership_writes_but_not_database_copies() {
        let _agent = TestAgentDir::new();
        let state = state();
        let settings_path = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(&settings_path, "{not-json").unwrap();

        assert!(ProviderService::add(&state, AppType::Pi, input("model-a"), true).is_err());
        assert!(state
            .db
            .get_provider_by_id("cc-switch-test", "pi")
            .unwrap()
            .is_none());
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());

        ProviderService::add(&state, AppType::Pi, input("model-a"), false)
            .expect("save a database-only copy");
        assert!(ProviderService::switch(&state, AppType::Pi, "cc-switch-test").is_err());
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
    }
}
