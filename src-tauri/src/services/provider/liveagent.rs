use super::{ProviderService, SwitchResult};
use crate::{
    app_config::AppType, error::AppError, liveagent_config as native, provider::Provider,
    store::AppState,
};
use indexmap::IndexMap;

const APP: &str = "liveagent";

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    // An unreadable config.json must not hide the saved catalog, and must not mark
    // every provider as removed from LiveAgent either.
    let live = match native::get_providers() {
        Ok(live) => live,
        Err(error) => {
            log::warn!("Failed to read LiveAgent providers; showing saved catalog: {error}");
            return state.db.get_all_providers(APP);
        }
    };
    let mut saved = state.db.get_all_providers(APP)?;
    for (id, config) in &live {
        let entry = saved
            .entry(id.clone())
            .or_insert_with(|| Provider::with_id(id.clone(), id.clone(), config.clone(), None));
        entry.settings_config = config.clone();
        if let Some(name) = config
            .get("name")
            .and_then(serde_json::Value::as_str)
            .filter(|s| !s.is_empty())
        {
            entry.name = name.into();
        }
    }
    for (id, provider) in &mut saved {
        ProviderService::set_provider_live_config_managed(provider, live.contains_key(id));
        state.db.save_provider(APP, provider)?;
    }
    Ok(saved)
}

fn prepare(provider: &mut Provider) -> Result<(), AppError> {
    native::validate_provider(&provider.id, &provider.settings_config)?;
    provider.settings_config["name"] = serde_json::json!(provider.name);
    provider.in_failover_queue = false;
    ProviderService::normalize_usage_script_credential_overrides(&AppType::LiveAgent, provider);
    Ok(())
}

pub(super) fn add(
    state: &AppState,
    mut provider: Provider,
    enabled: bool,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    prepare(&mut provider)?;
    if state.db.get_provider_by_id(&provider.id, APP)?.is_some() {
        return Err(AppError::InvalidInput(
            "LiveAgent provider key already exists".into(),
        ));
    }
    ProviderService::set_provider_live_config_managed(&mut provider, enabled);
    native::update_and_commit(
        |doc| {
            let providers = native::provider_map(doc)?;
            if providers.contains_key(&provider.id) {
                return Err(AppError::InvalidInput(
                    "LiveAgent provider key already exists in config.json".into(),
                ));
            }
            if enabled {
                providers.insert(provider.id.clone(), provider.settings_config.clone());
            }
            Ok(())
        },
        || {
            state.db.save_provider(APP, &provider)?;
            Ok(true)
        },
    )
}

pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    mut provider: Provider,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    if original_id.is_some_and(|id| id != provider.id) {
        return Err(AppError::InvalidInput(
            "LiveAgent provider keys cannot be renamed".into(),
        ));
    }
    if state.db.get_provider_by_id(&provider.id, APP)?.is_none() {
        return Err(AppError::InvalidInput(
            "LiveAgent provider not found".into(),
        ));
    }
    prepare(&mut provider)?;
    let provider = std::cell::RefCell::new(provider);
    native::update_and_commit(
        |doc| {
            let mut provider = provider.borrow_mut();
            let present = native::provider_map(doc)?.contains_key(&provider.id);
            ProviderService::set_provider_live_config_managed(&mut provider, present);
            if present {
                native::check_routes(doc, &provider.id, Some(&provider.settings_config))?;
                native::provider_map(doc)?
                    .insert(provider.id.clone(), provider.settings_config.clone());
            }
            Ok(())
        },
        || {
            state.db.save_provider(APP, &provider.borrow())?;
            Ok(true)
        },
    )
}

pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let mut provider = state
        .db
        .get_provider_by_id(id, APP)?
        .ok_or_else(|| AppError::InvalidInput("LiveAgent provider not found".into()))?;
    native::validate_provider(id, &provider.settings_config)?;
    ProviderService::set_provider_live_config_managed(&mut provider, true);
    let provider = std::cell::RefCell::new(provider);
    native::update_and_commit(
        |doc| {
            let mut provider = provider.borrow_mut();
            let providers = native::provider_map(doc)?;
            if let Some(current) = providers.get(id) {
                provider.settings_config = current.clone();
            } else {
                providers.insert(id.into(), provider.settings_config.clone());
            }
            Ok(())
        },
        || {
            state.db.save_provider(APP, &provider.borrow())?;
            Ok(SwitchResult::default())
        },
    )
}

pub(super) fn remove(state: &AppState, id: &str, delete: bool) -> Result<(), AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app(APP));
    let Some(mut provider) = state.db.get_provider_by_id(id, APP)? else {
        return Ok(());
    };
    ProviderService::set_provider_live_config_managed(&mut provider, false);
    let provider = std::cell::RefCell::new(provider);
    native::update_and_commit(
        |doc| {
            let mut provider = provider.borrow_mut();
            if native::provider_map(doc)?.contains_key(id) {
                native::check_routes(doc, id, None)?;
            }
            if let Some(current) = native::provider_map(doc)?.remove(id) {
                provider.settings_config = current;
            }
            Ok(())
        },
        || {
            if delete {
                state.db.delete_provider(APP, id)?;
            } else {
                state.db.save_provider(APP, &provider.borrow())?;
            }
            Ok(())
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::Database;
    use serde_json::json;
    use std::{fs, sync::Arc};

    fn provider(id: &str) -> Provider {
        Provider::with_id(
            id.into(),
            id.into(),
            json!({"api":"openai-responses","baseUrl":"https://example.com/v1","apiKey":"test-secret","models":[{"id":"model","contextWindow":32000}],"activeModels":["model"]}),
            None,
        )
    }
    #[test]
    #[serial_test::serial]
    fn liveagent_provider_crud_preserves_domains_and_protects_routes() {
        let root = native::test_support::TestRoot::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        fs::write(
            root.dir.path().join("config.json"),
            "// comment\n{\"language\":\"zh_cn\",\"mcp\":{\"docs\":{\"command\":[\"node\"]}}}",
        )
        .unwrap();
        add(&state, provider("first"), true).unwrap();
        add(&state, provider("second"), false).unwrap();
        assert!(add(&state, provider("first"), true).is_err());
        native::set_default("first", "model").unwrap();
        assert!(remove(&state, "first", true).is_err());
        let mut changed = provider("first");
        changed.settings_config["apiKey"] = json!("new-key");
        update(&state, None, changed).unwrap();
        assert_eq!(
            native::read().unwrap()["mcp"]["docs"]["command"],
            json!(["node"])
        );
        enable(&state, "second").unwrap();
        native::set_default("second", "model").unwrap();
        remove(&state, "first", false).unwrap();
        assert!(state.db.get_provider_by_id("first", APP).unwrap().is_some());
        assert!(!native::get_providers().unwrap().contains_key("first"));
        enable(&state, "first").unwrap();
        assert_eq!(
            native::get_providers().unwrap()["first"]["apiKey"],
            "new-key"
        );
        remove(&state, "first", true).unwrap();
        assert!(state.db.get_provider_by_id("first", APP).unwrap().is_none());
        let saved = list(&state).unwrap();
        assert!(saved["second"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed
            .unwrap());
        assert_eq!(native::read().unwrap()["language"], "zh_cn");
    }
    #[test]
    #[serial_test::serial]
    fn unreadable_config_keeps_catalog_and_managed_flags() {
        let root = native::test_support::TestRoot::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let path = root.dir.path().join("config.json");
        fs::write(&path, "{\"providers\":{}}").unwrap();
        add(&state, provider("kept"), true).unwrap();
        fs::write(&path, "{ broken").unwrap();
        let listed = list(&state).unwrap();
        assert!(listed["kept"]
            .meta
            .as_ref()
            .unwrap()
            .live_config_managed
            .unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ broken");
    }

    #[test]
    #[serial_test::serial]
    fn liveagent_provider_db_failure_restores_file() {
        let root = native::test_support::TestRoot::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let path = root.dir.path().join("config.json");
        fs::write(&path, "// keep\n{\"providers\":{}}").unwrap();
        let before = fs::read(&path).unwrap();
        state.db.conn.lock().unwrap().execute_batch("CREATE TRIGGER reject_liveagent BEFORE INSERT ON providers BEGIN SELECT RAISE(ABORT,'test failure'); END;").unwrap();
        assert!(add(&state, provider("p"), true).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
}
