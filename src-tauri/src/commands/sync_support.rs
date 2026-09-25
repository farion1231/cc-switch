use serde_json::{json, Value};

use crate::error::AppError;
use crate::services::{model_pricing, PromptService, ProviderService};
use crate::settings;
use crate::store::AppState;

pub(crate) fn run_post_import_sync(app_state: &AppState) -> Result<(), AppError> {
    let mut failures = Vec::new();
    if let Err(error) = crate::claude_launcher_profile::reconcile_profiles(app_state) {
        failures.push(format!("Claude launch profiles: {error}"));
    }

    if let Err(error) = ProviderService::sync_current_to_live(app_state) {
        failures.push(format!("live configuration: {error}"));
    }
    if let Err(error) = PromptService::sync_all_to_live(app_state) {
        failures.push(format!("prompts: {error}"));
    }
    if let Err(error) = model_pricing::sync_local_model_pricing(&app_state.db) {
        failures.push(format!("model pricing: {error}"));
    }
    if let Err(error) = settings::reload_settings() {
        failures.push(format!("settings cache: {error}"));
    }

    match app_state.db.get_log_config() {
        Ok(log_config) => log::set_max_level(log_config.to_level_filter()),
        Err(error) => {
            log::set_max_level(log::LevelFilter::Info);
            failures.push(format!("runtime log level: {error}"));
        }
    }
    app_state.usage_cache.invalidate_all();

    if failures.is_empty() {
        Ok(())
    } else {
        Err(AppError::Message(format!(
            "部分导入后同步失败: {}",
            failures.join("; ")
        )))
    }
}

fn post_sync_warning<E: std::fmt::Display>(err: E) -> String {
    AppError::localized(
        "sync.post_operation_sync_failed",
        format!("后置同步状态失败: {err}"),
        format!("Post-operation synchronization failed: {err}"),
    )
    .to_string()
}

pub(crate) fn post_sync_warning_from_result(
    result: Result<Result<(), AppError>, String>,
) -> Option<String> {
    match result {
        Ok(Ok(())) => None,
        Ok(Err(err)) => Some(post_sync_warning(err)),
        Err(err) => Some(post_sync_warning(err)),
    }
}

pub(crate) fn attach_warning(mut value: Value, warning: Option<String>) -> Value {
    if let Some(message) = warning {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("warning".to_string(), Value::String(message));
        }
    }
    value
}

pub(crate) fn success_payload_with_warning(backup_id: String, warning: Option<String>) -> Value {
    attach_warning(
        json!({
            "success": true,
            "message": "SQL imported successfully",
            "backupId": backup_id
        }),
        warning,
    )
}

#[cfg(test)]
mod tests {
    use super::{attach_warning, post_sync_warning_from_result, run_post_import_sync};
    use crate::app_config::AppType;
    use crate::claude_launcher_profile::{list_profiles, sync_profile};
    use crate::database::Database;
    use crate::provider::Provider;
    use crate::store::AppState;
    use serde_json::json;
    use serial_test::serial;
    use std::ffi::OsString;
    use std::sync::Arc;

    struct TestHome {
        _temp: tempfile::TempDir,
        previous_home: Option<OsString>,
        previous_settings: crate::settings::AppSettings,
    }

    impl TestHome {
        fn new() -> Self {
            let previous_home = std::env::var_os("CC_SWITCH_TEST_HOME");
            let previous_settings = crate::settings::get_settings();
            let temp = tempfile::tempdir().expect("create test home");
            std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
            crate::settings::update_settings(crate::settings::AppSettings::default())
                .expect("reset isolated settings");
            Self {
                _temp: temp,
                previous_home,
                previous_settings,
            }
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            crate::settings::update_settings(self.previous_settings.clone())
                .expect("restore settings");
            match &self.previous_home {
                Some(value) => std::env::set_var("CC_SWITCH_TEST_HOME", value),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    #[test]
    fn post_sync_warning_from_result_returns_none_on_success() {
        let warning = post_sync_warning_from_result(Ok(Ok(())));
        assert!(warning.is_none());
    }

    #[test]
    fn post_sync_warning_from_result_returns_some_on_sync_error() {
        let warning =
            post_sync_warning_from_result(Ok(Err(crate::error::AppError::Config("boom".into()))));
        assert!(warning.is_some());
    }

    #[tokio::test]
    async fn post_sync_warning_from_result_returns_some_on_join_error() {
        let handle = tokio::spawn(async move {
            panic!("forced join error");
        });
        let join_err = handle.await.expect_err("task should panic");
        let warning = post_sync_warning_from_result(Err(join_err.to_string()));
        assert!(warning.is_some());
    }

    #[test]
    fn attach_warning_adds_warning_without_dropping_existing_fields() {
        let payload = json!({ "status": "downloaded" });
        let updated = attach_warning(payload, Some("post sync warning".to_string()));
        assert_eq!(
            updated.get("status").and_then(|v| v.as_str()),
            Some("downloaded")
        );
        assert_eq!(
            updated.get("warning").and_then(|v| v.as_str()),
            Some("post sync warning")
        );
    }

    #[test]
    #[serial]
    fn post_import_sync_retires_profiles_missing_from_restored_database() {
        let _home = TestHome::new();
        let db = Arc::new(Database::memory().expect("create memory database"));
        let state = AppState::new(db.clone());
        let provider = Provider::with_id(
            "provider-a".to_string(),
            "Provider A".to_string(),
            json!({
                "env": {
                    "ANTHROPIC_AUTH_TOKEN": "secret-a",
                    "ANTHROPIC_BASE_URL": "https://provider-a.example"
                }
            }),
            None,
        );
        db.save_provider(AppType::Claude.as_str(), &provider)
            .expect("save provider A");
        let dir = sync_profile(&state, &provider).expect("create profile A");
        db.delete_provider(AppType::Claude.as_str(), "provider-a")
            .expect("simulate restored database without provider A");

        let _ = run_post_import_sync(&state);

        assert!(!dir.join("settings.json").exists());
        assert!(!dir.join(".claude.json").exists());
        assert!(list_profiles().expect("list profiles")[0].metadata.retired);
    }
}
