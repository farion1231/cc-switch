//! Shared catalog-only renaming for Pi and Oh My Pi.

use super::provider_exists_in_live_config;
use crate::{app_config::AppType, error::AppError, provider::Provider, store::AppState};

/// Call with a validated provider while holding the application's switch lock.
/// Native configuration, defaults and credentials are never rewritten.
pub(super) fn rename(
    state: &AppState,
    app_type: &AppType,
    original_id: &str,
    provider: &Provider,
) -> Result<bool, AppError> {
    let (label, config_file) = match app_type {
        AppType::Pi => ("Pi", "models.json"),
        AppType::Omp => ("Oh My Pi", "models.yml"),
        _ => {
            return Err(AppError::InvalidInput(
                "Pi-family renaming only supports Pi and Oh My Pi".into(),
            ));
        }
    };

    if provider_exists_in_live_config(app_type, original_id)? {
        return Err(AppError::InvalidInput(format!(
            "{label} provider keys cannot be renamed while the provider is added to {config_file}"
        )));
    }
    if state
        .db
        .get_provider_by_id(&provider.id, app_type.as_str())?
        .is_some()
        || provider_exists_in_live_config(app_type, &provider.id)?
    {
        return Err(AppError::InvalidInput(format!(
            "{label} provider '{}' already exists",
            provider.id
        )));
    }

    // The transaction also moves endpoint/health references and rolls back
    // both IDs if any step fails, instead of exposing a temporary duplicate.
    state
        .db
        .replace_provider_id(app_type.as_str(), original_id, provider)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{database::Database, services::provider::ProviderService};
    use serde_json::json;
    use std::{fs, path::PathBuf, sync::Arc};

    fn for_each_app(mut test: impl FnMut(AppType, &AppState)) {
        let _pi = crate::pi_config::test_support::TestAgentDir::new();
        let _omp = crate::omp_config::test_support::TestAgentDir::new();
        for app in [AppType::Pi, AppType::Omp] {
            let state = AppState::new(Arc::new(Database::memory().unwrap()));
            test(app, &state);
        }
    }

    fn provider(id: &str) -> Provider {
        Provider::with_id(
            id.into(),
            "Saved provider".into(),
            json!({"baseUrl": "https://example.com/v1", "apiKey": "test-key", "api": "openai-completions", "models": [{"id": "a"}]}),
            None,
        )
    }

    fn native_path(app: &AppType) -> PathBuf {
        match app {
            AppType::Pi => crate::pi_config::get_pi_models_path().unwrap(),
            AppType::Omp => crate::omp_config::get_omp_models_path().unwrap(),
            _ => unreachable!(),
        }
    }

    #[test]
    #[serial_test::serial]
    fn both_apps_allow_renaming_only_after_removal_without_changing_native_files() {
        for_each_app(|app, state| {
            let original = provider("source-provider");
            let renamed = provider("renamed-provider");
            ProviderService::add(state, app.clone(), original.clone(), true).unwrap();
            let path = native_path(&app);
            let live_bytes = fs::read(&path).unwrap();

            let error =
                ProviderService::update(state, app.clone(), Some(&original.id), renamed.clone())
                    .unwrap_err();
            assert!(error.to_string().contains("cannot be renamed"));
            assert_eq!(fs::read(&path).unwrap(), live_bytes);
            assert!(state
                .db
                .get_provider_by_id(&renamed.id, app.as_str())
                .unwrap()
                .is_none());

            ProviderService::remove_from_live_config(state, app.clone(), &original.id).unwrap();
            let removed_bytes = fs::read(&path).unwrap();
            // The other app's catalog must be unaffected by this rename.
            let other = if app == AppType::Pi { "omp" } else { "pi" };
            state.db.save_provider(other, &original).unwrap();
            ProviderService::update(state, app.clone(), Some(&original.id), renamed.clone())
                .unwrap();

            assert_eq!(fs::read(path).unwrap(), removed_bytes);
            assert!(state
                .db
                .get_provider_by_id(&original.id, app.as_str())
                .unwrap()
                .is_none());
            assert!(state
                .db
                .get_provider_by_id(&renamed.id, app.as_str())
                .unwrap()
                .is_some());
            assert!(state
                .db
                .get_provider_by_id(&original.id, other)
                .unwrap()
                .is_some());
        });
    }

    #[test]
    #[serial_test::serial]
    fn both_apps_reject_catalog_and_unsynced_native_key_collisions() {
        for_each_app(|app, state| {
            let original = provider("source-provider");
            let target = provider("taken-provider");
            ProviderService::add(state, app.clone(), original.clone(), false).unwrap();
            ProviderService::add(state, app.clone(), target.clone(), false).unwrap();
            let error =
                ProviderService::update(state, app.clone(), Some(&original.id), target.clone())
                    .unwrap_err();
            assert!(error.to_string().contains("already exists"));

            state.db.delete_provider(app.as_str(), &target.id).unwrap();
            ProviderService::add(state, app.clone(), target.clone(), true).unwrap();
            state.db.delete_provider(app.as_str(), &target.id).unwrap();
            let path = native_path(&app);
            let native_bytes = fs::read(&path).unwrap();
            let error = ProviderService::update(state, app.clone(), Some(&original.id), target)
                .unwrap_err();
            assert!(error.to_string().contains("already exists"));
            assert_eq!(fs::read(path).unwrap(), native_bytes);
            assert!(state
                .db
                .get_provider_by_id(&original.id, app.as_str())
                .unwrap()
                .is_some());
        });
    }

    #[test]
    #[serial_test::serial]
    fn both_apps_roll_back_the_whole_rename_when_deleting_the_original_fails() {
        for_each_app(|app, state| {
            let original = provider("source-provider");
            let renamed = provider("renamed-provider");
            ProviderService::add(state, app.clone(), original.clone(), false).unwrap();
            state
                .db
                .conn
                .lock()
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER fail_rename BEFORE DELETE ON providers
                 WHEN OLD.id = 'source-provider'
                 BEGIN SELECT RAISE(ABORT, 'forced rename failure'); END;",
                )
                .unwrap();

            let error =
                ProviderService::update(state, app.clone(), Some(&original.id), renamed.clone())
                    .unwrap_err();
            assert!(error.to_string().contains("forced rename failure"));
            assert!(state
                .db
                .get_provider_by_id(&original.id, app.as_str())
                .unwrap()
                .is_some());
            assert!(state
                .db
                .get_provider_by_id(&renamed.id, app.as_str())
                .unwrap()
                .is_none());
            assert!(!native_path(&app).exists());
        });
    }

    #[test]
    #[serial_test::serial]
    fn both_apps_preserve_endpoint_references_when_renaming() {
        for_each_app(|app, state| {
            let original = provider("source-provider");
            let renamed = provider("renamed-provider");
            ProviderService::add(state, app.clone(), original.clone(), false).unwrap();
            state
                .db
                .add_custom_endpoint(app.as_str(), &original.id, "https://backup.example/v1")
                .unwrap();
            ProviderService::update(state, app.clone(), Some(&original.id), renamed.clone())
                .unwrap();
            let saved = state.db.get_all_providers(app.as_str()).unwrap();
            assert!(saved[&renamed.id]
                .meta
                .as_ref()
                .unwrap()
                .custom_endpoints
                .contains_key("https://backup.example/v1"));
        });
    }
}
