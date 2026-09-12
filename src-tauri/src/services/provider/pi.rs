use super::{ProviderService, SwitchResult};
use crate::app_config::AppType;
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta, UsageScript};
use crate::store::AppState;
use indexmap::IndexMap;
use serde_json::Value;

const PI_APP: &str = "pi";

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(PI_APP));
    match crate::pi_config::read_pi_native_providers() {
        Ok(native) => {
            if let Err(error) = sync_native_locked(state, &native) {
                log::warn!("Failed to sync Pi providers from native config: {error}");
            }
        }
        Err(error) => {
            log::warn!("Failed to read Pi providers; showing saved catalog: {error}");
        }
    }
    state.db.get_all_providers(PI_APP)
}

pub(super) fn import_from_live(state: &AppState) -> Result<usize, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(PI_APP));
    let native = crate::pi_config::read_pi_native_providers()?;
    sync_native_locked(state, &native)
}

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
    align_native_display_name(&mut provider);
    ProviderService::normalize_usage_script_credential_overrides(&app_type, &mut provider);

    if state
        .db
        .get_provider_by_id(&provider.id, app_type.as_str())?
        .is_some()
    {
        return Err(AppError::InvalidInput(format!(
            "Pi provider '{}' already exists",
            provider.id
        )));
    }

    if !add_to_live && crate::pi_config::pi_provider_exists(&provider.id)? {
        return Err(AppError::InvalidInput(format!(
            "Pi provider key '{}' already exists in models.json",
            provider.id
        )));
    }

    let native_inserted = if add_to_live {
        crate::pi_config::insert_pi_provider(&provider.id, &provider.settings_config)?
    } else {
        false
    };

    if let Err(error) = state.db.save_provider(app_type.as_str(), &provider) {
        if native_inserted {
            if let Err(rollback) = crate::pi_config::remove_pi_provider_if_matches(
                &provider.id,
                &provider.settings_config,
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

pub(super) fn update_usage_script(
    state: &AppState,
    id: &str,
    script: UsageScript,
) -> Result<bool, AppError> {
    let app_type = AppType::Pi;
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(app_type.as_str()));
    super::validate_usage_script(&script)?;

    let mut provider = state
        .db
        .get_provider_by_id(id, app_type.as_str())?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{id}' not found")))?;
    provider
        .meta
        .get_or_insert_with(ProviderMeta::default)
        .usage_script = Some(script);
    strip_unsupported_pi_metadata(&mut provider);
    ProviderService::normalize_usage_script_credential_overrides(&app_type, &mut provider);
    state.db.save_provider(app_type.as_str(), &provider)?;
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

    state
        .db
        .get_provider_by_id(&original_id, app_type.as_str())?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{original_id}' not found")))?;
    strip_unsupported_pi_metadata(&mut provider);
    ProviderService::validate_provider_settings(&app_type, &provider)?;
    ProviderService::normalize_usage_script_credential_overrides(&app_type, &mut provider);

    let previous_native =
        crate::pi_config::replace_pi_provider_if_present(&original_id, &provider.settings_config)?;
    if let Err(error) = state.db.save_provider(app_type.as_str(), &provider) {
        if let Some(previous_native) = previous_native.as_ref() {
            if let Err(rollback) = crate::pi_config::replace_pi_provider(
                &original_id,
                &provider.settings_config,
                previous_native,
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
    let Some(_) = state.db.get_provider_by_id(id, app_type.as_str())? else {
        return Ok(());
    };
    // Delete is intentionally keyed by provider ID. Once the user confirms
    // deleting the provider itself, supported field edits do not change that
    // intent; the latest native value is retained only for rollback.
    let removed = crate::pi_config::remove_pi_provider(id)?;

    if let Err(error) = state.db.delete_provider(app_type.as_str(), id) {
        if let Some(removed) = removed.as_ref() {
            if let Err(rollback) = crate::pi_config::restore_pi_provider_if_missing(id, removed) {
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
    let provider = state
        .db
        .get_provider_by_id(id, app_type.as_str())?
        .ok_or_else(|| AppError::InvalidInput(format!("Pi provider '{id}' not found")))?;
    let Some(removed) = crate::pi_config::remove_pi_provider(id)? else {
        return Ok(());
    };
    let mut synced = provider;
    merge_native_config(&mut synced, removed.clone());
    if let Err(error) = state.db.save_provider(app_type.as_str(), &synced) {
        if let Err(rollback) = crate::pi_config::restore_pi_provider_if_missing(id, &removed) {
            return Err(AppError::Config(format!(
                "failed to preserve Pi provider before removal: {error}; native rollback failed: {rollback}"
            )));
        }
        return Err(error);
    }
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

    if let Some(native) = crate::pi_config::read_pi_native_provider(id)? {
        let mut synced = provider;
        merge_native_config(&mut synced, native);
        state.db.save_provider(app_type.as_str(), &synced)?;
        return Ok(SwitchResult::default());
    }

    ProviderService::validate_provider_settings(&app_type, &provider)?;
    crate::pi_config::insert_pi_provider(id, &provider.settings_config)?;
    Ok(SwitchResult::default())
}

fn sync_native_locked(
    state: &AppState,
    native: &IndexMap<String, Value>,
) -> Result<usize, AppError> {
    let saved = state.db.get_all_providers(PI_APP)?;
    let mut changed = 0;

    for (id, config) in native {
        let mut provider = saved.get(id).cloned().unwrap_or_else(|| {
            let name = native_provider_name(config).unwrap_or(id).to_string();
            let mut imported = Provider::with_id(id.clone(), name, config.clone(), None);
            imported.category = Some("custom".to_string());
            imported.icon = Some("pi".to_string());
            imported
        });
        let is_new = !saved.contains_key(id);
        let previous_name = provider.name.clone();
        let previous_config = provider.settings_config.clone();
        merge_native_config(&mut provider, config.clone());
        if !is_new && provider.name == previous_name && provider.settings_config == previous_config
        {
            continue;
        }

        state.db.save_provider(PI_APP, &provider)?;
        changed += 1;
    }

    Ok(changed)
}

fn merge_native_config(provider: &mut Provider, config: Value) {
    if let Some(name) = native_provider_name(&config) {
        provider.name = name.to_string();
    }
    provider.settings_config = config;
}

fn native_provider_name(config: &Value) -> Option<&str> {
    config
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
}

fn align_native_display_name(provider: &mut Provider) {
    let Some(config) = provider.settings_config.as_object_mut() else {
        return;
    };
    if config.contains_key("name") {
        config.insert("name".to_string(), Value::String(provider.name.clone()));
    }
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

    fn usage_script(code: &str) -> UsageScript {
        UsageScript {
            enabled: true,
            language: "javascript".to_string(),
            code: code.to_string(),
            timeout: Some(5),
            api_key: None,
            base_url: None,
            access_token: None,
            user_id: None,
            template_type: None,
            auto_query_interval: Some(10),
            coding_plan_provider: None,
            access_key_id: None,
            secret_access_key: None,
            team_organization_id: None,
            team_project_id: None,
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
    fn default_selection_does_not_block_membership_changes() {
        let _agent = TestAgentDir::new();
        let state = state();
        let original = input("model-a");
        ProviderService::add(&state, AppType::Pi, original.clone(), true).expect("add provider");
        let settings_path = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(
            &settings_path,
            r#"{"defaultProvider":"cc-switch-test","defaultModel":"model-a"}"#,
        )
        .unwrap();

        update(&state, Some("cc-switch-test"), input("model-b"))
            .expect("global default must not block model edits");
        ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
            .expect("global default must not block removal");
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());

        ProviderService::switch(&state, AppType::Pi, "cc-switch-test").expect("re-enable provider");
        ProviderService::delete(&state, AppType::Pi, "cc-switch-test")
            .expect("global default must not block deletion");
        assert!(state
            .db
            .get_provider_by_id("cc-switch-test", "pi")
            .unwrap()
            .is_none());
        assert_eq!(
            fs::read_to_string(settings_path).unwrap(),
            r#"{"defaultProvider":"cc-switch-test","defaultModel":"model-a"}"#
        );
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
    }

    #[test]
    #[serial]
    fn provider_membership_never_changes_pi_auth_or_defaults() {
        let _agent = TestAgentDir::new();
        let state = state();
        let agent_dir = crate::pi_config::get_pi_agent_dir().expect("agent directory");
        fs::create_dir_all(&agent_dir).expect("create agent directory");
        let auth_path = agent_dir.join("auth.json");
        let settings_path = agent_dir.join("settings.json");
        let auth_contents = br#"{
            "anthropic": {"type":"oauth","refresh":"native-secret"},
            "openai": {"type":"api_key","key":"native-api-key"}
        }"#;
        let settings_contents =
            br#"{"defaultProvider":"anthropic","defaultModel":"claude-opus-4-6"}"#;
        fs::write(&auth_path, auth_contents).expect("write auth");
        fs::write(&settings_path, settings_contents).expect("write settings");
        let models_path = agent_dir.join("models.json");
        fs::write(
            &models_path,
            r#"{"providers":{"anthropic":{"futureField":{"keep":true}}}}"#,
        )
        .expect("write explicit provider");

        ProviderService::list(&state, AppType::Pi).expect("import explicit provider");
        ProviderService::remove_from_live_config(&state, AppType::Pi, "anthropic")
            .expect("remove explicit provider");
        ProviderService::switch(&state, AppType::Pi, "anthropic")
            .expect("enable explicit provider");
        let mut edited = state
            .db
            .get_provider_by_id("anthropic", PI_APP)
            .expect("read provider")
            .expect("provider");
        edited.settings_config["anotherField"] = json!(true);
        update(&state, Some("anthropic"), edited).expect("edit explicit provider");

        assert_eq!(fs::read(auth_path).expect("read auth"), auth_contents);
        assert_eq!(
            fs::read(settings_path).expect("read settings"),
            settings_contents
        );
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
    fn native_edits_sync_to_the_saved_provider_and_survive_removal() {
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
        external["models"][0]["contextWindow"] = json!(1_000_000.0);
        crate::pi_config::replace_pi_provider("cc-switch-test", &saved.settings_config, &external)
            .expect("edit native provider");

        let listed = ProviderService::list(&state, AppType::Pi).expect("sync native providers");
        assert_eq!(listed["cc-switch-test"].name, "External edit");
        assert_eq!(listed["cc-switch-test"].settings_config, external);

        ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
            .expect("remove externally edited provider");
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
        let preserved = state
            .db
            .get_provider_by_id("cc-switch-test", "pi")
            .unwrap()
            .unwrap();
        assert_eq!(preserved.name, "External edit");
        assert_eq!(preserved.settings_config, external);
    }

    #[test]
    #[serial]
    fn refreshed_native_config_can_be_edited_without_snapshot_state() {
        let _agent = TestAgentDir::new();
        let state = state();
        let baseline = input("model-a");
        ProviderService::add(&state, AppType::Pi, baseline.clone(), true).expect("add provider");

        let mut external = baseline.settings_config.clone();
        external["apiKey"] = json!("rotated-outside");
        external["futureField"] = json!({ "preserve": true });
        crate::pi_config::replace_pi_provider(
            "cc-switch-test",
            &baseline.settings_config,
            &external,
        )
        .expect("edit native provider");

        let listed = ProviderService::list(&state, AppType::Pi).expect("refresh native provider");
        let mut local = listed["cc-switch-test"].clone();
        local.name = "Local edit".to_string();
        local.settings_config["name"] = json!("Local edit");
        update(&state, Some("cc-switch-test"), local).expect("edit the refreshed native provider");
        assert_eq!(
            crate::pi_config::read_pi_native_provider("cc-switch-test")
                .expect("read native provider")
                .expect("native provider")["futureField"],
            json!({ "preserve": true })
        );
    }

    #[test]
    #[serial]
    fn enabled_provider_edit_needs_no_special_snapshot_parameter() {
        let _agent = TestAgentDir::new();
        let state = state();
        ProviderService::add(&state, AppType::Pi, input("model-a"), true).expect("add provider");

        let mut edited = input("model-b");
        edited.settings_config["unknownField"] = json!({ "keep": true });
        ProviderService::update(&state, AppType::Pi, Some("cc-switch-test"), edited.clone())
            .expect("edit enabled provider");

        assert_eq!(
            crate::pi_config::read_pi_native_provider("cc-switch-test")
                .expect("read native provider")
                .expect("native provider"),
            edited.settings_config
        );
    }

    #[test]
    #[serial]
    fn native_sync_imports_every_explicit_provider_node() {
        let _agent = TestAgentDir::new();
        let state = state();
        let mut stale_oauth = input("stale-model");
        stale_oauth.id = "native-oauth".to_string();
        ProviderService::add(&state, AppType::Pi, stale_oauth, false).expect("save stale provider");
        let path = crate::pi_config::get_pi_models_path().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            r#"{
                "providers": {
                    "native-custom": {
                        "name": "Native custom",
                        "baseUrl": "https://api.example.com/v1",
                        "apiKey": "secret",
                        "api": "openai-completions",
                        "models": [{ "id": "model-a" }]
                    },
                    "anthropic": {
                        "name": "Built in",
                        "baseUrl": "https://api.anthropic.com",
                        "api": "anthropic-messages",
                        "models": [{ "id": "claude" }]
                    },
                    "openai": {},
                    "deepseek": {
                        "futureField": { "preserve": true }
                    },
                    "native-oauth": {
                        "name": "OAuth",
                        "oauth": "example",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-b" }]
                    }
                }
            }"#,
        )
        .unwrap();

        let providers = ProviderService::list(&state, AppType::Pi).expect("sync providers");
        assert_eq!(providers.len(), 5);
        let imported = &providers["native-custom"];
        assert_eq!(imported.name, "Native custom");
        assert_eq!(imported.category.as_deref(), Some("custom"));
        assert_eq!(imported.icon.as_deref(), Some("pi"));
        assert_eq!(providers["anthropic"].name, "Built in");
        assert_eq!(providers["openai"].settings_config, json!({}));
        assert_eq!(
            providers["deepseek"].settings_config["futureField"],
            json!({ "preserve": true })
        );
        assert_eq!(
            providers["native-oauth"].settings_config["oauth"],
            json!("example")
        );
    }

    #[test]
    #[serial]
    fn removal_preserves_and_can_restore_a_minimal_native_node() {
        let _agent = TestAgentDir::new();
        let state = state();
        ProviderService::add(&state, AppType::Pi, input("model-a"), true).expect("add provider");
        let minimal = json!({
            "name": "Extension-owned provider",
            "extension": { "type": "custom" }
        });
        let path = crate::pi_config::get_pi_models_path().expect("models path");
        fs::write(
            &path,
            serde_json::to_vec(&json!({
                "providers": {
                    "cc-switch-test": minimal.clone()
                }
            }))
            .expect("serialize models"),
        )
        .expect("replace native provider");

        ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
            .expect("remove exact native node");
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
        assert_eq!(
            state
                .db
                .get_provider_by_id("cc-switch-test", PI_APP)
                .expect("read saved provider")
                .expect("saved provider")
                .settings_config,
            minimal
        );

        ProviderService::switch(&state, AppType::Pi, "cc-switch-test")
            .expect("restore the complete native node");
        assert_eq!(
            crate::pi_config::read_pi_native_provider("cc-switch-test")
                .expect("read restored provider"),
            Some(minimal)
        );
    }

    #[test]
    #[serial]
    fn usage_metadata_update_does_not_rewrite_native_provider_settings() {
        let _agent = TestAgentDir::new();
        let state = state();
        let baseline = input("model-a");
        ProviderService::add(&state, AppType::Pi, baseline.clone(), true).expect("add provider");

        let mut external = baseline.settings_config.clone();
        external["apiKey"] = json!("rotated-outside");
        external["futureField"] = json!({ "preserve": true });
        crate::pi_config::replace_pi_provider(
            "cc-switch-test",
            &baseline.settings_config,
            &external,
        )
        .expect("edit native provider");

        update_usage_script(&state, "cc-switch-test", usage_script("return {}"))
            .expect("save usage metadata");
        assert_eq!(
            crate::pi_config::read_pi_native_provider("cc-switch-test")
                .expect("read native provider")
                .expect("native provider"),
            external
        );

        let providers = ProviderService::list(&state, AppType::Pi).expect("sync provider");
        let saved = &providers["cc-switch-test"];
        assert_eq!(saved.settings_config, external);
        assert_eq!(
            saved
                .meta
                .as_ref()
                .and_then(|meta| meta.usage_script.as_ref())
                .map(|script| script.code.as_str()),
            Some("return {}")
        );
    }

    #[test]
    #[serial]
    fn copied_provider_keeps_its_display_name_after_enable_and_sync() {
        let _agent = TestAgentDir::new();
        let state = state();
        let mut copy = input("model-a");
        copy.id = "cc-switch-test-copy".to_string();
        copy.name = "Test provider copy".to_string();

        ProviderService::add(&state, AppType::Pi, copy, false).expect("save copied provider");
        ProviderService::switch(&state, AppType::Pi, "cc-switch-test-copy")
            .expect("enable copied provider");
        let providers = ProviderService::list(&state, AppType::Pi).expect("sync providers");

        assert_eq!(providers["cc-switch-test-copy"].name, "Test provider copy");
        assert_eq!(
            providers["cc-switch-test-copy"].settings_config["name"],
            json!("Test provider copy")
        );
    }

    #[test]
    #[serial]
    fn database_only_create_does_not_overwrite_an_unsynced_native_key() {
        let _agent = TestAgentDir::new();
        let state = state();
        let path = crate::pi_config::get_pi_models_path().expect("models path");
        fs::create_dir_all(path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(
            &path,
            r#"{
                "providers": {
                    "cc-switch-test-copy": {
                        "name": "Native OAuth",
                        "oauth": "example",
                        "baseUrl": "https://api.example.com/v1",
                        "api": "openai-completions",
                        "models": [{ "id": "model-a" }]
                    }
                }
            }"#,
        )
        .expect("write native provider");

        let mut copy = input("model-a");
        copy.id = "cc-switch-test-copy".to_string();
        let error = ProviderService::add(&state, AppType::Pi, copy, false)
            .expect_err("an unsynced native provider key must stay reserved");

        assert!(error.to_string().contains("already exists in models.json"));
        assert!(state
            .db
            .get_provider_by_id("cc-switch-test-copy", PI_APP)
            .expect("read saved provider")
            .is_none());
        assert!(crate::pi_config::pi_provider_exists("cc-switch-test-copy")
            .expect("read native provider"));

        let providers = ProviderService::list(&state, AppType::Pi).expect("sync native provider");
        assert_eq!(providers["cc-switch-test-copy"].name, "Native OAuth");
    }

    #[test]
    #[serial]
    fn malformed_native_file_keeps_the_saved_catalog_visible() {
        let _agent = TestAgentDir::new();
        let state = state();
        ProviderService::add(&state, AppType::Pi, input("model-a"), false).expect("save provider");
        let path = crate::pi_config::get_pi_models_path().expect("models path");
        fs::create_dir_all(path.parent().expect("models directory"))
            .expect("create models directory");
        fs::write(path, "{not-json").expect("write malformed models");

        let providers = ProviderService::list(&state, AppType::Pi).expect("read saved catalog");
        assert!(providers.contains_key("cc-switch-test"));
    }

    #[test]
    #[serial]
    fn numeric_json_representation_does_not_block_removal() {
        let _agent = TestAgentDir::new();
        let state = state();
        let mut saved = input("model-a");
        saved.settings_config["models"][0]["contextWindow"] = json!(1_000_000);
        ProviderService::add(&state, AppType::Pi, saved, false).expect("save provider");

        let path = crate::pi_config::get_pi_models_path().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            path,
            r#"{
                "providers": {
                    "cc-switch-test": {
                        "name": "Test provider",
                        "baseUrl": "https://api.example.com/v1",
                        "apiKey": "secret",
                        "api": "openai-completions",
                        "models": [{ "id": "model-a", "contextWindow": 1000000.0 }]
                    }
                }
            }"#,
        )
        .unwrap();

        ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
            .expect("remove provider with equivalent numeric representation");
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
        assert_eq!(
            state
                .db
                .get_provider_by_id("cc-switch-test", "pi")
                .unwrap()
                .unwrap()
                .settings_config["models"][0]["contextWindow"]
                .as_f64(),
            Some(1_000_000.0)
        );
    }

    #[test]
    #[serial]
    fn unreadable_selection_does_not_block_membership_changes() {
        let _agent = TestAgentDir::new();
        let state = state();
        let settings_path = crate::pi_config::get_pi_settings_path().unwrap();
        fs::create_dir_all(settings_path.parent().unwrap()).unwrap();
        fs::write(&settings_path, "{not-json").unwrap();

        ProviderService::add(&state, AppType::Pi, input("model-a"), true)
            .expect("selection is unrelated to adding a provider");
        assert!(state
            .db
            .get_provider_by_id("cc-switch-test", "pi")
            .unwrap()
            .is_some());
        assert!(crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());

        let original = input("model-a");
        update(&state, Some("cc-switch-test"), original.clone())
            .expect("an edit that keeps every model does not need the default selection");

        ProviderService::remove_from_live_config(&state, AppType::Pi, "cc-switch-test")
            .expect("global selection is advisory for removal");
        ProviderService::switch(&state, AppType::Pi, "cc-switch-test").expect("re-enable provider");
        ProviderService::delete(&state, AppType::Pi, "cc-switch-test")
            .expect("global selection is advisory for deletion");
        assert!(!crate::pi_config::pi_provider_exists("cc-switch-test").unwrap());
    }
}
