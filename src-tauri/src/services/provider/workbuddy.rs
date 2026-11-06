//! WorkBuddy providers: one card per entry of `~/.workbuddy-ai/models.json`.
//!
//! WorkBuddy owns the active model itself (it is picked per conversation in its
//! UI). CC Switch only manages the stored model list, so this adapter is
//! additive: cards coexist in the same file and can be shown or hidden
//! independently. "Enable" means the model is listed in `availableModels`
//! (visible in WorkBuddy's picker), never "the model WorkBuddy is using".

use super::{ProviderService, SwitchResult};
use crate::error::AppError;
use crate::provider::{Provider, ProviderMeta};
use crate::store::AppState;
use crate::workbuddy_config;
use indexmap::IndexMap;
use serde_json::Value;

const WORKBUDDY_APP: &str = "workbuddy";
const WORKBUDDY_ICON: &str = "workbuddy";

fn set_managed(provider: &mut Provider, managed: bool) {
    ProviderService::set_provider_live_config_managed(provider, managed);
}

fn prepare(provider: &mut Provider) -> Result<(), AppError> {
    workbuddy_config::validate_model_node(&provider.id, &provider.settings_config)?;
    provider.in_failover_queue = false;
    // WorkBuddy has no local routing, no MCP sync and no request rewriting, so every
    // other provider flag would be a promise CC Switch cannot keep.
    provider.meta = provider.meta.take().map(|meta| ProviderMeta {
        usage_script: meta.usage_script,
        is_partner: meta.is_partner,
        partner_promotion_key: meta.partner_promotion_key,
        live_config_managed: meta.live_config_managed,
        ..Default::default()
    });
    Ok(())
}

fn model_name(config: &Value) -> Option<&str> {
    config
        .get("name")
        .and_then(Value::as_str)
        .filter(|name| !name.trim().is_empty())
}

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(WORKBUDDY_APP));
    match workbuddy_config::read_snapshot() {
        Ok(snapshot) => {
            if let Err(error) = sync_native(state, &snapshot) {
                log::warn!("Failed to sync WorkBuddy models from models.json: {error}");
            }
        }
        // An unreadable file must not empty the page: the saved catalog still shows
        // what CC Switch knows, and the first mutation reports the real error.
        Err(error) => log::warn!("Failed to read WorkBuddy models; showing saved catalog: {error}"),
    }
    state.db.get_all_providers(WORKBUDDY_APP)
}

pub(super) fn import_from_live(state: &AppState) -> Result<usize, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(WORKBUDDY_APP));
    let snapshot = workbuddy_config::read_snapshot()?;
    sync_native(state, &snapshot)
}

fn sync_native(
    state: &AppState,
    snapshot: &workbuddy_config::ModelsSnapshot,
) -> Result<usize, AppError> {
    let saved = state.db.get_all_providers(WORKBUDDY_APP)?;
    let mut changed = 0;

    for (id, config) in &snapshot.models {
        let is_new = !saved.contains_key(id);
        let mut provider = saved.get(id).cloned().unwrap_or_else(|| {
            let name = model_name(config).unwrap_or(id).to_string();
            let mut imported = Provider::with_id(id.clone(), name, config.clone(), None);
            imported.category = Some("custom".to_string());
            imported.icon = Some(WORKBUDDY_ICON.to_string());
            imported
        });

        let previous_name = provider.name.clone();
        let previous_config = provider.settings_config.clone();
        let previous_managed = provider
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed);
        let managed = workbuddy_config::is_available(snapshot.available.as_deref(), id);

        if let Some(name) = model_name(config) {
            provider.name = name.to_string();
        }
        provider.settings_config = config.clone();
        set_managed(&mut provider, managed);

        if !is_new
            && provider.name == previous_name
            && provider.settings_config == previous_config
            && previous_managed == Some(managed)
        {
            continue;
        }
        state.db.save_provider(WORKBUDDY_APP, &provider)?;
        changed += 1;
    }

    // A model removed directly in WorkBuddy is absent from the native
    // snapshot. Keep its saved card so it can be added again, but clear the
    // native-membership flag or the UI will keep treating it as enabled.
    for (id, saved_provider) in &saved {
        if snapshot.models.contains_key(id) {
            continue;
        }
        if saved_provider
            .meta
            .as_ref()
            .and_then(|meta| meta.live_config_managed)
            == Some(true)
        {
            let mut provider = saved_provider.clone();
            set_managed(&mut provider, false);
            state.db.save_provider(WORKBUDDY_APP, &provider)?;
            changed += 1;
        }
    }

    Ok(changed)
}

pub(super) fn add(
    state: &AppState,
    mut provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(WORKBUDDY_APP));
    prepare(&mut provider)?;

    if state
        .db
        .get_provider_by_id(&provider.id, WORKBUDDY_APP)?
        .is_some()
    {
        return Err(AppError::InvalidInput(format!(
            "WorkBuddy model '{}' already exists",
            provider.id
        )));
    }
    if !add_to_live && workbuddy_config::model_exists(&provider.id)? {
        return Err(AppError::InvalidInput(format!(
            "WorkBuddy model id '{}' already exists in models.json",
            provider.id
        )));
    }

    let native_inserted = if add_to_live {
        workbuddy_config::insert_model(&provider.id, &provider.settings_config)?
    } else {
        false
    };

    // "Managed" describes the native file, so it can only be true once the model
    // is actually an entry of `models`. A card saved without `add_to_live` has
    // nothing in the file yet, and a file without `availableModels` lists all of
    // its models — reading the visibility back covers both cases.
    let managed = if add_to_live {
        match workbuddy_config::read_available_models() {
            Ok(available) => workbuddy_config::is_available(available.as_deref(), &provider.id),
            Err(error) => {
                if native_inserted {
                    if let Err(rollback) = workbuddy_config::remove_model_if_matches(
                        &provider.id,
                        &provider.settings_config,
                    ) {
                        return Err(AppError::Config(format!(
                            "failed to save WorkBuddy model: {error}; native rollback failed: {rollback}"
                        )));
                    }
                }
                return Err(error);
            }
        }
    } else {
        false
    };
    set_managed(&mut provider, managed);

    if let Err(error) = state.db.save_provider(WORKBUDDY_APP, &provider) {
        if native_inserted {
            if let Err(rollback) =
                workbuddy_config::remove_model_if_matches(&provider.id, &provider.settings_config)
            {
                return Err(AppError::Config(format!(
                    "failed to save WorkBuddy model: {error}; native rollback failed: {rollback}"
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
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(WORKBUDDY_APP));
    let original_id = original_id.unwrap_or(&provider.id).to_string();
    if original_id != provider.id {
        // The id is the model name WorkBuddy sends upstream and the key both the
        // models array and availableModels use; renaming in place would silently
        // leave a dangling visibility entry behind.
        return Err(AppError::InvalidInput(
            "WorkBuddy model ids cannot be renamed; delete the model and add it again".to_string(),
        ));
    }

    if state
        .db
        .get_provider_by_id(&original_id, WORKBUDDY_APP)?
        .is_none()
    {
        return Err(AppError::InvalidInput(format!(
            "WorkBuddy model '{original_id}' not found"
        )));
    }
    prepare(&mut provider)?;

    let previous_native =
        workbuddy_config::replace_model_if_present(&original_id, &provider.settings_config)?;
    // Editing a card that has no native entry must not report the model as
    // managed: nothing was written, so WorkBuddy cannot be showing it.
    let managed = match &previous_native {
        None => false,
        Some(_) => match workbuddy_config::read_available_models() {
            Ok(available) => workbuddy_config::is_available(available.as_deref(), &original_id),
            Err(error) => return Err(error),
        },
    };
    set_managed(&mut provider, managed);

    if let Err(error) = state.db.save_provider(WORKBUDDY_APP, &provider) {
        if let Some(previous_native) = previous_native.as_ref() {
            if let Err(rollback) = workbuddy_config::replace_model(
                &original_id,
                &provider.settings_config,
                previous_native,
            ) {
                return Err(AppError::Config(format!(
                    "failed to save WorkBuddy model: {error}; native rollback failed: {rollback}"
                )));
            }
        }
        return Err(error);
    }
    Ok(true)
}

/// Delete the card and its `models[]` entry (and its visibility entry).
pub(super) fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(WORKBUDDY_APP));
    if state.db.get_provider_by_id(id, WORKBUDDY_APP)?.is_none() {
        return Ok(());
    }
    let removed = workbuddy_config::remove_model(id)?;

    if let Err(error) = state.db.delete_provider(WORKBUDDY_APP, id) {
        if let Some(removed) = removed.as_ref() {
            if let Err(rollback) =
                workbuddy_config::restore_model(id, &removed.config, removed.was_available)
            {
                return Err(AppError::Config(format!(
                    "failed to delete WorkBuddy model: {error}; native rollback failed: {rollback}"
                )));
            }
        }
        return Err(error);
    }
    Ok(())
}

/// Stop showing a model in WorkBuddy without losing its configuration.
pub(super) fn remove(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(WORKBUDDY_APP));
    let Some(mut provider) = state.db.get_provider_by_id(id, WORKBUDDY_APP)? else {
        return Ok(());
    };
    if workbuddy_config::model_exists(id)? {
        workbuddy_config::set_model_available(id, false)?;
    }
    set_managed(&mut provider, false);
    state.db.save_provider(WORKBUDDY_APP, &provider)?;
    Ok(())
}

/// Show a model in WorkBuddy, writing the entry first when it only exists as a card.
pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let _guard =
        futures::executor::block_on(state.proxy_service.lock_switch_for_app(WORKBUDDY_APP));
    let mut provider = state
        .db
        .get_provider_by_id(id, WORKBUDDY_APP)?
        .ok_or_else(|| AppError::InvalidInput(format!("WorkBuddy model '{id}' not found")))?;
    prepare(&mut provider)?;

    if workbuddy_config::model_exists(id)? {
        workbuddy_config::set_model_available(id, true)?;
    } else {
        workbuddy_config::insert_model(id, &provider.settings_config)?;
    }
    set_managed(&mut provider, true);
    state.db.save_provider(WORKBUDDY_APP, &provider)?;
    Ok(SwitchResult::default())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_config::AppType;
    use crate::database::Database;
    use crate::provider::UsageScript;
    use crate::workbuddy_config::test_support::TestConfigDir;
    use serde_json::json;
    use serial_test::serial;
    use std::fs;
    use std::sync::Arc;

    fn state() -> AppState {
        AppState::new(Arc::new(
            Database::memory().expect("create in-memory database"),
        ))
    }

    fn model(id: &str) -> Value {
        json!({
            "id": id,
            "name": format!("Model {id}"),
            "vendor": "custom",
            "apiKey": "test-key",
            "url": "https://api.example.com/v1/chat/completions",
            "maxInputTokens": 128000,
            "maxOutputTokens": 8192,
            "supportsToolCall": true,
            "supportsImages": false,
            "supportsReasoning": false,
            "relatedModels": []
        })
    }

    fn input(id: &str) -> Provider {
        Provider {
            id: id.to_string(),
            name: format!("Model {id}"),
            settings_config: model(id),
            website_url: None,
            category: Some("custom".to_string()),
            created_at: Some(1),
            sort_index: None,
            notes: None,
            meta: Some(ProviderMeta {
                common_config_enabled: Some(true),
                endpoint_auto_select: Some(true),
                api_format: Some("openai_chat".to_string()),
                custom_user_agent: Some("legacy-route-agent".to_string()),
                is_partner: Some(true),
                ..ProviderMeta::default()
            }),
            icon: None,
            icon_color: None,
            in_failover_queue: true,
        }
    }

    fn native_ids() -> Vec<String> {
        workbuddy_config::read_snapshot()
            .expect("read models")
            .models
            .keys()
            .cloned()
            .collect()
    }

    fn native_file() -> Value {
        serde_json::from_str(
            &fs::read_to_string(workbuddy_config::get_models_path().unwrap()).unwrap(),
        )
        .expect("parse models.json")
    }

    #[test]
    #[serial]
    fn add_without_live_only_saves_the_card() {
        let _dir = TestConfigDir::new();
        let state = state();

        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), false)
            .expect("save disabled model");

        assert!(!workbuddy_config::model_exists("model-a").unwrap());
        let saved = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .expect("saved card");
        assert_eq!(
            saved.meta.as_ref().and_then(|m| m.live_config_managed),
            Some(false)
        );
        // Provider-only flags WorkBuddy cannot honour are dropped.
        let meta = saved.meta.unwrap();
        assert_eq!(meta.common_config_enabled, None);
        assert_eq!(meta.endpoint_auto_select, None);
        assert_eq!(meta.api_format, None);
        assert_eq!(meta.custom_user_agent, None);
        assert_eq!(meta.is_partner, Some(true));
        assert!(!saved.in_failover_queue);
    }

    #[test]
    #[serial]
    fn add_to_live_writes_the_model_and_makes_it_visible() {
        let _dir = TestConfigDir::new();
        let state = state();

        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true)
            .expect("add model");

        assert_eq!(native_ids(), ["model-a"]);
        let file = native_file();
        assert_eq!(file["models"][0]["apiKey"], "test-key");
        let saved = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .expect("saved card");
        assert_eq!(
            saved.meta.as_ref().and_then(|m| m.live_config_managed),
            Some(true)
        );
    }

    #[test]
    #[serial]
    fn duplicate_ids_are_rejected_before_touching_the_file() {
        let _dir = TestConfigDir::new();
        let state = state();
        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true)
            .expect("add model");

        assert!(ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), false).is_err());
        assert_eq!(native_ids(), ["model-a"]);
    }

    #[test]
    #[serial]
    fn update_edits_the_native_entry_and_keeps_unknown_fields() {
        let _dir = TestConfigDir::new();
        let state = state();
        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true)
            .expect("add model");

        // A field WorkBuddy added later must survive a CC Switch edit.
        let path = workbuddy_config::get_models_path().unwrap();
        let mut document: Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        document["models"][0]["futureField"] = json!({"keep": true});
        fs::write(&path, serde_json::to_string_pretty(&document).unwrap()).unwrap();

        ProviderService::list(&state, AppType::WorkBuddy).expect("sync native edit");
        let mut edited = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .unwrap();
        edited.settings_config["name"] = json!("Edited name");

        ProviderService::update(&state, AppType::WorkBuddy, Some("model-a"), edited)
            .expect("edit model");

        let file = native_file();
        assert_eq!(file["models"][0]["name"], "Edited name");
        assert_eq!(file["models"][0]["futureField"], json!({"keep": true}));
        // The file never had the key, so editing must not invent one: WorkBuddy
        // reads a missing list as "every model is available".
        assert!(file.get("availableModels").is_none());
    }

    #[test]
    #[serial]
    fn updating_a_card_only_model_keeps_it_unmanaged() {
        let _dir = TestConfigDir::new();
        let state = state();
        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), false)
            .expect("save disabled model");

        let mut edited = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .unwrap();
        edited.settings_config["name"] = json!("Edited name");
        ProviderService::update(&state, AppType::WorkBuddy, Some("model-a"), edited)
            .expect("edit card");

        // Nothing was written to models.json, so the model is still unmanaged.
        assert!(!workbuddy_config::model_exists("model-a").unwrap());
        let saved = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.meta.as_ref().and_then(|m| m.live_config_managed),
            Some(false)
        );
    }

    #[test]
    #[serial]
    fn update_refuses_renaming_the_model_id() {
        let _dir = TestConfigDir::new();
        let state = state();
        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true)
            .expect("add model");
        let mut edited = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .unwrap();
        edited.id = "model-b".to_string();

        let error = ProviderService::update(&state, AppType::WorkBuddy, Some("model-a"), edited)
            .expect_err("rename must be refused");
        assert!(error.to_string().contains("cannot be renamed"));
        assert_eq!(native_ids(), ["model-a"]);
    }

    #[test]
    #[serial]
    fn delete_removes_the_entry_and_the_card() {
        let _dir = TestConfigDir::new();
        let state = state();
        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true)
            .expect("add model");
        // Give the file a visibility list so deletion has one to clean up, and a
        // foreign id that no model owns so the cleanup has to be surgical.
        let path = workbuddy_config::get_models_path().unwrap();
        let mut document: Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        document["availableModels"] = json!(["model-a", "foreign"]);
        fs::write(&path, serde_json::to_string_pretty(&document).unwrap()).unwrap();

        ProviderService::delete(&state, AppType::WorkBuddy, "model-a").expect("delete model");

        assert!(native_ids().is_empty());
        assert!(state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .is_none());
        assert_eq!(native_file()["availableModels"], json!(["foreign"]));
    }

    #[test]
    #[serial]
    fn remove_hides_the_model_but_keeps_it_configured() {
        let _dir = TestConfigDir::new();
        let state = state();
        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true)
            .expect("add model");

        ProviderService::remove_from_live_config(&state, AppType::WorkBuddy, "model-a")
            .expect("hide model");

        // Still configured, only no longer offered by WorkBuddy.
        assert_eq!(native_ids(), ["model-a"]);
        assert_eq!(native_file()["availableModels"], json!([]));
        let saved = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .unwrap();
        assert_eq!(
            saved.meta.as_ref().and_then(|m| m.live_config_managed),
            Some(false)
        );

        ProviderService::switch(&state, AppType::WorkBuddy, "model-a").expect("show model again");
        assert_eq!(native_file()["availableModels"], json!(["model-a"]));
    }

    #[test]
    #[serial]
    fn list_reports_visibility_from_the_file() {
        let _dir = TestConfigDir::new();
        let state = state();
        let path = workbuddy_config::get_models_path().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(
            &path,
            serde_json::to_string(&json!({
                "models": [model("visible"), model("hidden")],
                "availableModels": ["visible"]
            }))
            .unwrap(),
        )
        .unwrap();

        let providers = ProviderService::list(&state, AppType::WorkBuddy).expect("list models");
        assert_eq!(providers.len(), 2);
        assert_eq!(
            providers["visible"]
                .meta
                .as_ref()
                .and_then(|m| m.live_config_managed),
            Some(true)
        );
        assert_eq!(
            providers["hidden"]
                .meta
                .as_ref()
                .and_then(|m| m.live_config_managed),
            Some(false)
        );
        assert_eq!(
            ProviderService::current(&state, AppType::WorkBuddy).unwrap(),
            ""
        );
    }

    #[test]
    #[serial]
    fn native_deletion_retains_card_and_allows_readding() {
        let _dir = TestConfigDir::new();
        let state = state();
        ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true).unwrap();
        workbuddy_config::remove_model("model-a").unwrap();
        let providers = ProviderService::list(&state, AppType::WorkBuddy).unwrap();
        assert_eq!(providers["model-a"].meta.as_ref().unwrap().live_config_managed, Some(false));
        ProviderService::switch(&state, AppType::WorkBuddy, "model-a").unwrap();
        assert!(workbuddy_config::model_exists("model-a").unwrap());
    }

    #[test]
    #[serial]
    fn failed_native_write_does_not_leave_a_card_behind() {
        let _dir = TestConfigDir::new();
        let state = state();
        let path = workbuddy_config::get_models_path().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "{ not json").unwrap();

        assert!(ProviderService::add(&state, AppType::WorkBuddy, input("model-a"), true).is_err());
        assert!(state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .is_none());
    }

    #[test]
    #[serial]
    fn usage_scripts_survive_the_provider_metadata_strip() {
        let _dir = TestConfigDir::new();
        let state = state();
        let mut provider = input("model-a");
        provider.meta.as_mut().unwrap().usage_script = Some(UsageScript {
            enabled: true,
            language: "javascript".to_string(),
            code: "return null".to_string(),
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
        });

        ProviderService::add(&state, AppType::WorkBuddy, provider, false).expect("add model");
        let saved = state
            .db
            .get_provider_by_id("model-a", "workbuddy")
            .unwrap()
            .unwrap();
        assert!(saved
            .meta
            .as_ref()
            .and_then(|meta| meta.usage_script.as_ref())
            .is_some());
    }
}
