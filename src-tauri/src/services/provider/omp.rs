use super::{ProviderService, SwitchResult};
use crate::{
    app_config::AppType,
    error::AppError,
    omp_config::{self, Change},
    provider::{Provider, ProviderMeta},
    store::AppState,
};
use indexmap::IndexMap;

fn prepare(provider: &mut Provider) -> Result<(), AppError> {
    omp_config::validate_provider(&provider.id, &provider.settings_config)?;
    provider.in_failover_queue = false;
    provider.meta = provider.meta.take().map(|meta| ProviderMeta {
        usage_script: meta.usage_script,
        is_partner: meta.is_partner,
        partner_promotion_key: meta.partner_promotion_key,
        ..Default::default()
    });
    Ok(())
}

pub(super) fn list(state: &AppState) -> Result<IndexMap<String, Provider>, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    if let Err(error) = sync_native(state) {
        log::warn!("Failed to read Oh My Pi providers; showing saved catalog: {error}");
    }
    let mut providers = state.db.get_all_providers("omp")?;
    match omp_config::get_native_oauth_provider_ids() {
        Ok(ids) => {
            for id in ids {
                providers.entry(id.clone()).or_insert_with(|| {
                    let icon = if id == "openai-codex" {
                        "openai"
                    } else {
                        "omp"
                    };
                    let mut provider =
                        Provider::with_id(id.clone(), id, serde_json::json!({}), None);
                    provider.category = Some("official".into());
                    provider.icon = Some(icon.into());
                    provider.meta = Some(ProviderMeta {
                        native_login: Some(true),
                        ..Default::default()
                    });
                    provider
                });
            }
        }
        Err(error) => log::warn!("Failed to read Oh My Pi native login metadata: {error}"),
    }
    Ok(providers)
}
fn sync_native(state: &AppState) -> Result<usize, AppError> {
    let native = omp_config::get_providers()?;
    let saved = state.db.get_all_providers("omp")?;
    let mut changed = 0;
    for (id, config) in native {
        let mut provider = saved.get(&id).cloned().unwrap_or_else(|| {
            let name = config
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or(&id);
            let mut provider = Provider::with_id(id.clone(), name.into(), config.clone(), None);
            provider.category = Some("custom".into());
            provider.icon = Some("omp".into());
            provider
        });
        if saved.contains_key(&id) && provider.settings_config == config {
            continue;
        }
        provider.settings_config = config;
        state.db.save_provider("omp", &provider)?;
        changed += 1;
    }
    Ok(changed)
}
pub(super) fn import_from_live(state: &AppState) -> Result<usize, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    sync_native(state)
}
pub(super) fn add(
    state: &AppState,
    mut provider: Provider,
    add_to_live: bool,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    prepare(&mut provider)?;
    if state.db.get_provider_by_id(&provider.id, "omp")?.is_some() {
        return Err(AppError::Config("omp.errors.duplicateProvider".into()));
    }
    let commit = |_| {
        state.db.save_provider("omp", &provider)?;
        Ok(true)
    };
    if add_to_live {
        omp_config::change_and_commit(
            &provider.id,
            Some(&provider.settings_config),
            Change::Insert,
            commit,
        )
    } else {
        if omp_config::get_providers()?.contains_key(&provider.id) {
            return Err(AppError::Config("omp.errors.duplicateProvider".into()));
        }
        commit(None)
    }
}
pub(super) fn update(
    state: &AppState,
    original_id: Option<&str>,
    mut provider: Provider,
) -> Result<bool, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    if original_id.is_some_and(|id| id != provider.id) {
        return Err(AppError::Config("omp.errors.renameProvider".into()));
    }
    state
        .db
        .get_provider_by_id(&provider.id, "omp")?
        .ok_or_else(|| AppError::Config("omp.errors.providerMissing".into()))?;
    prepare(&mut provider)?;
    omp_config::change_and_commit(
        &provider.id,
        Some(&provider.settings_config),
        Change::ReplaceIfPresent,
        |_| {
            state.db.save_provider("omp", &provider)?;
            Ok(true)
        },
    )
}
pub(super) fn delete(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    state
        .db
        .get_provider_by_id(id, "omp")?
        .ok_or_else(|| AppError::Config("omp.errors.providerMissing".into()))?;
    omp_config::change_and_commit(id, None, Change::Remove, |_| {
        state.db.delete_provider("omp", id)
    })
}
pub(super) fn remove(state: &AppState, id: &str) -> Result<(), AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    let mut provider = state
        .db
        .get_provider_by_id(id, "omp")?
        .ok_or_else(|| AppError::Config("omp.errors.providerMissing".into()))?;
    omp_config::change_and_commit(id, None, Change::Remove, |old| {
        if let Some(config) = old {
            provider.settings_config = config;
            state.db.save_provider("omp", &provider)?;
        }
        Ok(())
    })
}
pub(super) fn enable(state: &AppState, id: &str) -> Result<SwitchResult, AppError> {
    let _guard = futures::executor::block_on(state.proxy_service.lock_switch_for_app("omp"));
    let mut provider = state
        .db
        .get_provider_by_id(id, "omp")?
        .ok_or_else(|| AppError::Config("omp.errors.providerMissing".into()))?;
    if let Some(native) = omp_config::get_providers()?.get(id) {
        provider.settings_config = native.clone();
        state.db.save_provider("omp", &provider)?;
        return Ok(SwitchResult::default());
    }
    ProviderService::validate_provider_settings(&AppType::Omp, &provider)?;
    omp_config::change_and_commit(id, Some(&provider.settings_config), Change::Insert, |_| {
        Ok(SwitchResult::default())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        database::Database,
        omp_config::test_support::{BrokerUrlEnv, TestAgentDir},
    };
    use serde_json::json;
    use std::{fs, sync::Arc};
    fn provider() -> Provider {
        Provider::with_id(
            "cc-switch-test".into(),
            "Test".into(),
            json!({"baseUrl":"https://example.com/v1","apiKey":"!literal credential","api":"openai-completions","models":[{"id":"a"}]}),
            None,
        )
    }
    #[test]
    #[serial_test::serial]
    fn written_round_trip_keeps_comments_and_unmanaged_nodes() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let path = omp_config::get_omp_models_path().unwrap();
        let before = "# user header\nnotes: 'verbatim'\nproviders:\n  other:\n    baseUrl: https://other.example # keep\n\n# user footer\n";
        fs::write(&path, before).unwrap();
        add(&state, provider(), true).unwrap();
        let mut edited = provider();
        edited.settings_config["models"][0]["id"] = json!("b");
        edited.settings_config["apiKey"] = json!("cc-switch-validation-key");
        update(&state, None, edited).unwrap();
        let written = fs::read_to_string(&path).unwrap();
        assert!(written.starts_with("# user header\nnotes: 'verbatim'\nproviders:\n  other:\n    baseUrl: https://other.example # keep\n"));
        assert!(written.ends_with("\n# user footer\n"));
        assert_eq!(
            omp_config::get_providers().unwrap()["cc-switch-test"]["models"][0]["id"],
            "b"
        );
        remove(&state, "cc-switch-test").unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), before);
    }

    #[test]
    #[serial_test::serial]
    fn usage_metadata_update_preserves_native_settings_and_pi_provider() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let baseline = provider();
        add(&state, baseline.clone(), true).unwrap();
        state.db.save_provider("pi", &baseline).unwrap();

        let mut external = baseline.settings_config.clone();
        external["apiKey"] = json!("rotated-outside");
        omp_config::change_and_commit(
            &baseline.id,
            Some(&external),
            Change::ReplaceIfPresent,
            |_| Ok(()),
        )
        .unwrap();
        let path = omp_config::get_omp_models_path().unwrap();
        let native_bytes = fs::read(&path).unwrap();
        let script = serde_json::from_value(json!({
            "enabled": true,
            "language": "javascript",
            "code": "return {}",
        }))
        .unwrap();

        ProviderService::update_pi_usage_script(&state, AppType::Omp, &baseline.id, script)
            .unwrap();
        assert_eq!(fs::read(&path).unwrap(), native_bytes);
        let saved = list(&state).unwrap();
        let saved = &saved[&baseline.id];
        assert_eq!(saved.settings_config, external);
        assert_eq!(
            saved
                .meta
                .as_ref()
                .unwrap()
                .usage_script
                .as_ref()
                .unwrap()
                .code,
            "return {}"
        );
        let pi = state
            .db
            .get_provider_by_id(&baseline.id, "pi")
            .unwrap()
            .unwrap();
        assert_eq!(pi.settings_config, baseline.settings_config);
        assert!(pi.meta.and_then(|meta| meta.usage_script).is_none());
    }

    #[test]
    #[serial_test::serial]
    fn membership_add_update_remove_enable_delete_never_touches_defaults_or_login() {
        let _agent = TestAgentDir::new();
        let dir = omp_config::get_omp_agent_dir().unwrap();
        let defaults = "# user's defaults\nmodelRoles:\n  # null\n";
        fs::write(dir.join("config.yml"), defaults).unwrap();
        fs::write(dir.join("agent.db"), b"native login").unwrap();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        ProviderService::add(&state, AppType::Omp, provider(), true).unwrap();
        assert!(omp_config::get_providers()
            .unwrap()
            .contains_key("cc-switch-test"));
        let mut changed = provider();
        changed.settings_config["models"][0]["id"] = json!("b");
        ProviderService::update(&state, AppType::Omp, None, changed).unwrap();
        assert_eq!(
            omp_config::get_providers().unwrap()["cc-switch-test"]["models"][0]["id"],
            "b"
        );
        ProviderService::remove_from_live_config(&state, AppType::Omp, "cc-switch-test").unwrap();
        assert!(omp_config::get_providers().unwrap().is_empty());
        ProviderService::switch(&state, AppType::Omp, "cc-switch-test").unwrap();
        assert_eq!(
            omp_config::get_providers().unwrap()["cc-switch-test"]["models"][0]["id"],
            "b"
        );
        ProviderService::delete(&state, AppType::Omp, "cc-switch-test").unwrap();
        assert!(omp_config::get_providers().unwrap().is_empty());
        assert_eq!(
            fs::read_to_string(dir.join("config.yml")).unwrap(),
            defaults
        );
        assert_eq!(fs::read(dir.join("agent.db")).unwrap(), b"native login");
    }
    #[test]
    #[serial_test::serial]
    fn db_failure_restores_native_bytes_and_catalog_only_add_does_not_write() {
        let _agent = TestAgentDir::new();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        add(&state, provider(), false).unwrap();
        assert!(!omp_config::get_omp_models_path().unwrap().exists());
        enable(&state, "cc-switch-test").unwrap();
        let path = omp_config::get_omp_models_path().unwrap();
        let bytes = fs::read(&path).unwrap();
        state
            .db
            .conn
            .lock()
            .unwrap()
            .execute_batch("PRAGMA query_only = ON")
            .unwrap();
        assert!(remove(&state, "cc-switch-test").is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        assert!(delete(&state, "cc-switch-test").is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }

    fn native_login_database() -> rusqlite::Connection {
        let path = omp_config::get_omp_data_dir().unwrap().join("agent.db");
        let conn = rusqlite::Connection::open(path).unwrap();
        conn.execute_batch(
            "CREATE TABLE auth_credentials (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                provider TEXT NOT NULL,
                credential_type TEXT NOT NULL,
                data TEXT NOT NULL,
                disabled_cause TEXT DEFAULT NULL,
                identity_key TEXT DEFAULT NULL,
                created_at INTEGER NOT NULL DEFAULT 0,
                updated_at INTEGER NOT NULL DEFAULT 0
            );
            INSERT INTO auth_credentials (provider, credential_type, data) VALUES
                ('openai-codex', 'oauth', '{\"refresh\":\"private-oauth-token\"}'),
                ('openai-codex', 'oauth', '{\"refresh\":\"other-private-token\"}'),
                ('deepseek', 'api_key', '{\"key\":\"private-api-key\"}');
            INSERT INTO auth_credentials (provider, credential_type, data, disabled_cause)
                VALUES ('anthropic', 'oauth', '{}', 'deleted by user');",
        )
        .unwrap();
        conn
    }

    #[test]
    #[serial_test::serial]
    fn native_oauth_cards_follow_logout_without_persisting_credentials() {
        let _agent = TestAgentDir::new();
        let _broker = BrokerUrlEnv::set(None);
        let conn = native_login_database();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        add(&state, provider(), true).unwrap();
        let models_path = omp_config::get_omp_models_path().unwrap();
        let models_before = fs::read(&models_path).unwrap();
        let auth_path = omp_config::get_omp_data_dir().unwrap().join("agent.db");
        let auth_before = fs::read(&auth_path).unwrap();

        let cards = ProviderService::list(&state, AppType::Omp).unwrap();
        assert_eq!(
            cards.keys().map(String::as_str).collect::<Vec<_>>(),
            vec!["cc-switch-test", "openai-codex"]
        );
        let card = serde_json::to_value(&cards["openai-codex"]).unwrap();
        assert_eq!(card["meta"]["nativeLogin"], true);
        assert_eq!(card["settingsConfig"], json!({}));
        let response = serde_json::to_string(&cards).unwrap();
        assert!(!response.contains("private-oauth-token"));
        assert!(!response.contains("other-private-token"));
        assert!(!response.contains("private-api-key"));
        assert!(state
            .db
            .get_provider_by_id("openai-codex", "omp")
            .unwrap()
            .is_none());
        assert_eq!(list(&state).unwrap().len(), 2);
        assert_eq!(fs::read(&models_path).unwrap(), models_before);
        assert_eq!(fs::read(&auth_path).unwrap(), auth_before);

        conn.execute(
            "UPDATE auth_credentials SET disabled_cause = 'deleted by user' WHERE provider = 'openai-codex'",
            [],
        )
        .unwrap();
        let cards = list(&state).unwrap();
        assert!(!cards.contains_key("openai-codex"));
        assert!(cards.contains_key("cc-switch-test"));
        assert_eq!(fs::read(models_path).unwrap(), models_before);
    }

    #[test]
    #[serial_test::serial]
    fn native_oauth_does_not_replace_saved_or_explicit_configuration() {
        let _agent = TestAgentDir::new();
        let _broker = BrokerUrlEnv::set(None);
        let _conn = native_login_database();
        let state = AppState::new(Arc::new(Database::memory().unwrap()));
        let mut saved = provider();
        saved.id = "openai-codex".into();
        saved.name = "My custom Codex".into();
        add(&state, saved.clone(), false).unwrap();

        let cards = list(&state).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards["openai-codex"].name, "My custom Codex");
        assert_eq!(cards["openai-codex"].settings_config, saved.settings_config);
        assert!(
            serde_json::to_value(&cards["openai-codex"]).unwrap()["meta"]["nativeLogin"].is_null()
        );

        enable(&state, "openai-codex").unwrap();
        let mut edited = saved.clone();
        edited.settings_config["models"][0]["id"] = json!("edited");
        update(&state, None, edited.clone()).unwrap();
        assert_eq!(
            list(&state).unwrap()["openai-codex"].settings_config,
            edited.settings_config
        );
        delete(&state, "openai-codex").unwrap();
        let projected = list(&state).unwrap();
        assert_eq!(
            serde_json::to_value(&projected["openai-codex"]).unwrap()["meta"]["nativeLogin"],
            true
        );
        assert!(delete(&state, "openai-codex").is_err());
        assert!(enable(&state, "openai-codex").is_err());
        assert!(remove(&state, "openai-codex").is_err());
        assert!(update(&state, None, saved).is_err());
        assert!(state.db.get_all_providers("omp").unwrap().is_empty());
    }
}
