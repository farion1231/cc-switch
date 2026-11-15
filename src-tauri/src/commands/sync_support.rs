use std::future::Future;

use serde_json::{json, Value};

use crate::database::Database;
use crate::error::AppError;
use crate::services::sync_protocol::run_with_sync_lock;
use crate::services::{model_pricing, PromptService, ProviderService};
use crate::settings::{self, WebDavSyncStatus};
use crate::store::AppState;

/// A remote sync backend (WebDAV or S3). Both backends' Tauri commands run
/// the shared logic below; a backend only supplies its settings, service
/// calls, and messages.
pub(crate) trait SyncBackend {
    type Settings: Send + Sync + 'static;
    /// Used in the "connection ok" message and the log prefix.
    const NAME: &'static str;
    /// `(key, zh, en)` of the "not configured" error.
    const NOT_CONFIGURED: [&'static str; 3];
    /// `(key, zh, en)` of the "disabled" error.
    const DISABLED: [&'static str; 3];

    fn load() -> Option<Self::Settings>;
    fn store(settings: Option<Self::Settings>) -> Result<(), AppError>;
    fn store_status(status: WebDavSyncStatus) -> Result<(), AppError>;
    fn is_enabled(settings: &Self::Settings) -> bool;
    fn status_mut(settings: &mut Self::Settings) -> &mut WebDavSyncStatus;
    /// The credential the frontend sends empty when the user did not edit it.
    fn secret_mut(settings: &mut Self::Settings) -> &mut String;
    fn normalize_and_validate(settings: &mut Self::Settings) -> Result<(), AppError>;
    /// Keeps this backend's auto sync from reacting to DB writes while alive.
    fn suppress_auto_sync() -> impl Send;
    fn check_connection(
        settings: &Self::Settings,
    ) -> impl Future<Output = Result<(), AppError>> + Send + '_;
    fn upload<'a>(
        db: &'a Database,
        settings: &'a mut Self::Settings,
    ) -> impl Future<Output = Result<Value, AppError>> + Send + 'a;
    fn download<'a>(
        db: &'a Database,
        settings: &'a mut Self::Settings,
    ) -> impl Future<Output = Result<Value, AppError>> + Send + 'a;
    fn fetch_remote_info(
        settings: &Self::Settings,
    ) -> impl Future<Output = Result<Option<Value>, AppError>> + Send + '_;
}

fn localized_error([key, zh, en]: [&'static str; 3]) -> String {
    AppError::localized(key, zh, en).to_string()
}

fn persist_sync_error<B: SyncBackend>(settings: &mut B::Settings, error: &AppError, source: &str) {
    let status = B::status_mut(settings);
    status.last_error = Some(error.to_string());
    status.last_error_source = Some(source.to_string());
    let _ = B::store_status(status.clone());
}

fn require_enabled_settings<B: SyncBackend>() -> Result<B::Settings, String> {
    let settings = B::load().ok_or_else(|| localized_error(B::NOT_CONFIGURED))?;
    if !B::is_enabled(&settings) {
        return Err(localized_error(B::DISABLED));
    }
    Ok(settings)
}

fn resolve_secret_for_request<B: SyncBackend>(
    mut incoming: B::Settings,
    existing: Option<B::Settings>,
    preserve_empty_secret: bool,
) -> B::Settings {
    if let Some(mut existing_settings) = existing {
        if preserve_empty_secret && B::secret_mut(&mut incoming).is_empty() {
            *B::secret_mut(&mut incoming) = std::mem::take(B::secret_mut(&mut existing_settings));
        }
    }
    incoming
}

async fn run_download_with_lock<B, T, U, DownloadFut, Project, ProjectFut>(
    download: DownloadFut,
    project: Project,
) -> Result<U, AppError>
where
    B: SyncBackend,
    DownloadFut: Future<Output = Result<T, AppError>>,
    Project: FnOnce(T) -> ProjectFut,
    ProjectFut: Future<Output = Result<U, AppError>>,
{
    run_with_sync_lock(async {
        let result = {
            let _auto_sync_suppression = B::suppress_auto_sync();
            download.await?
        };
        project(result).await
    })
    .await
}

fn map_sync_result<T, F>(result: Result<T, AppError>, on_error: F) -> Result<T, String>
where
    F: FnOnce(&AppError),
{
    match result {
        Ok(value) => Ok(value),
        Err(err) => {
            on_error(&err);
            Err(err.to_string())
        }
    }
}

pub(crate) async fn test_connection<B: SyncBackend>(
    settings: B::Settings,
    preserve_empty_secret: Option<bool>,
) -> Result<Value, String> {
    let preserve_empty = preserve_empty_secret.unwrap_or(true);
    let resolved = resolve_secret_for_request::<B>(settings, B::load(), preserve_empty);
    B::check_connection(&resolved)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "success": true,
        "message": format!("{} connection ok", B::NAME)
    }))
}

pub(crate) async fn sync_upload<B: SyncBackend>(state: &AppState) -> Result<Value, String> {
    let db = state.db.clone();
    let mut settings = require_enabled_settings::<B>()?;

    let result = run_with_sync_lock(B::upload(&db, &mut settings)).await;
    map_sync_result(result, |error| {
        persist_sync_error::<B>(&mut settings, error, "manual")
    })
}

pub(crate) async fn sync_download<B: SyncBackend>(state: &AppState) -> Result<Value, String> {
    let db = state.db.clone();
    let app_state_for_sync = state.clone();
    let mut settings = require_enabled_settings::<B>()?;

    // Keep the derived live configuration refresh in the same global sync
    // operation. Otherwise another WebDAV/S3 restore can start after the DB
    // apply but before this snapshot has finished projecting its live files.
    let sync_result = run_download_with_lock::<B, _, _, _, _, _>(
        B::download(&db, &mut settings),
        |result| async move {
            let post_sync_result = tauri::async_runtime::spawn_blocking(move || {
                run_post_import_sync(&app_state_for_sync)
            })
            .await
            .map_err(|e| e.to_string());
            Ok((result, post_sync_result))
        },
    )
    .await;
    let (mut result, post_sync_result) = map_sync_result(sync_result, |error| {
        persist_sync_error::<B>(&mut settings, error, "manual")
    })?;

    // Post-download sync is best-effort: snapshot restore has already succeeded.
    let warning = post_sync_warning_from_result(post_sync_result);
    if let Some(msg) = warning.as_ref() {
        log::warn!("[{}] post-download sync warning: {msg}", B::NAME);
    }
    result = attach_warning(result, warning);

    Ok(result)
}

pub(crate) async fn sync_save_settings<B: SyncBackend>(
    settings: B::Settings,
    password_touched: Option<bool>,
) -> Result<Value, String> {
    let password_touched = password_touched.unwrap_or(false);
    let mut existing = B::load();
    // Preserve server-owned fields that the frontend does not manage
    let existing_status = existing.as_mut().map(|s| B::status_mut(s).clone());
    let mut sync_settings = resolve_secret_for_request::<B>(settings, existing, !password_touched);
    if let Some(status) = existing_status {
        *B::status_mut(&mut sync_settings) = status;
    }

    B::normalize_and_validate(&mut sync_settings).map_err(|e| e.to_string())?;
    B::store(Some(sync_settings)).map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

pub(crate) async fn sync_fetch_remote_info<B: SyncBackend>() -> Result<Value, String> {
    let settings = require_enabled_settings::<B>()?;
    let info = B::fetch_remote_info(&settings)
        .await
        .map_err(|e| e.to_string())?;
    Ok(info.unwrap_or(json!({ "empty": true })))
}

pub(crate) fn run_post_import_sync(app_state: &AppState) -> Result<(), AppError> {
    let mut failures = Vec::new();

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
    use super::{
        attach_warning, map_sync_result, persist_sync_error, post_sync_warning_from_result,
        require_enabled_settings, resolve_secret_for_request, run_download_with_lock, SyncBackend,
    };
    use crate::commands::{s3_sync::S3Backend, webdav_sync::WebDavBackend};
    use crate::error::AppError;
    use crate::services::sync_protocol::{run_with_sync_lock, sync_mutex};
    use crate::settings::{AppSettings, S3SyncSettings, WebDavSyncSettings};
    use serde::{de::DeserializeOwned, Serialize};
    use serde_json::json;
    use serial_test::serial;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    fn webdav_sample(enabled: bool) -> WebDavSyncSettings {
        WebDavSyncSettings {
            enabled,
            base_url: "https://dav.example.com/dav/".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        }
    }

    fn s3_sample(enabled: bool) -> S3SyncSettings {
        S3SyncSettings {
            enabled,
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            ..S3SyncSettings::default()
        }
    }

    fn use_test_home<B: SyncBackend>(name: &str) {
        let test_home = std::env::temp_dir().join(format!(
            "cc-switch-{}-sync-{name}-test",
            B::NAME.to_lowercase()
        ));
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);
        crate::settings::update_settings(AppSettings::default()).expect("reset settings");
    }

    /// Settings as JSON without the status, to compare everything else.
    fn without_status<S: Serialize>(settings: &S) -> serde_json::Value {
        let mut value = serde_json::to_value(settings).expect("serialize settings");
        value
            .as_object_mut()
            .expect("settings object")
            .remove("status");
        value
    }

    async fn download_suppression_starts_after_lock_acquisition<B: SyncBackend>(
        is_suppressed: fn() -> bool,
    ) {
        assert!(!is_suppressed());
        let guard = sync_mutex().lock().await;
        let download_entered = AtomicBool::new(false);
        let projection_entered = AtomicBool::new(false);
        let download = run_download_with_lock::<B, _, _, _, _, _>(
            async {
                download_entered.store(true, Ordering::SeqCst);
                assert!(is_suppressed());
                Ok::<(), AppError>(())
            },
            |_| async {
                projection_entered.store(true, Ordering::SeqCst);
                assert!(!is_suppressed());
                Ok::<(), AppError>(())
            },
        );
        tokio::pin!(download);

        assert!(
            tokio::time::timeout(Duration::from_millis(40), download.as_mut())
                .await
                .is_err(),
            "download must wait while another sync operation holds the global lock"
        );
        assert!(!download_entered.load(Ordering::SeqCst));
        assert!(!projection_entered.load(Ordering::SeqCst));
        assert!(
            !is_suppressed(),
            "local changes must remain observable while the download waits for the global lock"
        );

        drop(guard);
        tokio::time::timeout(Duration::from_secs(1), download.as_mut())
            .await
            .expect("download should start after lock release")
            .expect("download operation should complete");

        assert!(download_entered.load(Ordering::SeqCst));
        assert!(projection_entered.load(Ordering::SeqCst));
        assert!(!is_suppressed());
    }

    /// `secret_field` is the JSON name of the field the frontend sends empty
    /// when the user did not retype the secret.
    fn resolve_secret_for_request_honours_preserve_flag<B: SyncBackend>(
        sample: fn(bool) -> B::Settings,
        secret_field: &str,
    ) where
        B::Settings: Serialize + DeserializeOwned,
    {
        let secret_of = |settings: &B::Settings| {
            serde_json::to_value(settings).expect("serialize settings")[secret_field].clone()
        };
        let empty_secret = || {
            let mut value = serde_json::to_value(sample(true)).expect("serialize settings");
            value[secret_field] = json!("");
            serde_json::from_value::<B::Settings>(value).expect("deserialize settings")
        };

        let preserved = resolve_secret_for_request::<B>(empty_secret(), Some(sample(true)), true);
        assert_eq!(secret_of(&preserved), secret_of(&sample(true)));
        assert_eq!(without_status(&preserved), without_status(&sample(true)));

        let cleared = resolve_secret_for_request::<B>(empty_secret(), Some(sample(true)), false);
        assert_eq!(secret_of(&cleared), json!(""));
    }

    fn persist_sync_error_updates_status_without_overwriting_credentials<B: SyncBackend>(
        sample: fn(bool) -> B::Settings,
    ) where
        B::Settings: Serialize,
    {
        use_test_home::<B>("error-status");
        B::store(Some(sample(true))).expect("seed settings");

        let mut current = sample(true);
        persist_sync_error::<B>(
            &mut current,
            &AppError::Config("boom".to_string()),
            "manual",
        );

        let mut after = B::load().expect("read settings");
        assert_eq!(without_status(&after), without_status(&sample(true)));
        let status = B::status_mut(&mut after);
        assert!(
            status
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("boom"),
            "status error should be updated"
        );
        assert_eq!(status.last_error_source.as_deref(), Some("manual"));
    }

    fn require_enabled_settings_checks_enabled<B: SyncBackend>(sample: fn(bool) -> B::Settings)
    where
        B::Settings: Serialize,
    {
        use_test_home::<B>("enabled");
        B::store(Some(sample(false))).expect("seed disabled settings");
        let err = require_enabled_settings::<B>()
            .err()
            .expect("disabled settings should fail");
        assert!(
            err.contains("disabled") || err.contains("未启用"),
            "unexpected error: {err}"
        );

        B::store(Some(sample(true))).expect("seed enabled settings");
        let settings =
            require_enabled_settings::<B>().expect("enabled settings should be accepted");
        assert_eq!(without_status(&settings), without_status(&sample(true)));
    }

    /// Every behavior test runs for each backend, so a backend can only
    /// differ in its `SyncBackend` impl, never in the command logic.
    macro_rules! backend_tests {
        ($name:ident, $backend:ty, $sample:expr, $secret_field:expr, $is_suppressed:path) => {
            mod $name {
                use super::*;

                #[tokio::test]
                #[serial]
                async fn download_suppression_starts_after_lock_acquisition() {
                    super::download_suppression_starts_after_lock_acquisition::<$backend>(
                        $is_suppressed,
                    )
                    .await;
                }

                #[test]
                fn resolve_secret_for_request_honours_preserve_flag() {
                    super::resolve_secret_for_request_honours_preserve_flag::<$backend>(
                        $sample,
                        $secret_field,
                    );
                }

                #[test]
                #[serial]
                fn persist_sync_error_updates_status_without_overwriting_credentials() {
                    super::persist_sync_error_updates_status_without_overwriting_credentials::<
                        $backend,
                    >($sample);
                }

                #[test]
                #[serial]
                fn require_enabled_settings_checks_enabled() {
                    super::require_enabled_settings_checks_enabled::<$backend>($sample);
                }
            }
        };
    }

    backend_tests!(
        webdav,
        WebDavBackend,
        webdav_sample,
        "password",
        crate::services::webdav_auto_sync::is_auto_sync_suppressed
    );
    backend_tests!(
        s3,
        S3Backend,
        s3_sample,
        "secretAccessKey",
        crate::services::s3_auto_sync::is_auto_sync_suppressed
    );

    #[tokio::test]
    #[serial]
    async fn sync_mutex_serializes_concurrent_access() {
        let guard = sync_mutex().lock().await;
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_bg = Arc::clone(&acquired);

        let waiter = tokio::spawn(async move {
            let _inner_guard = sync_mutex().lock().await;
            acquired_bg.store(true, Ordering::SeqCst);
        });

        tokio::time::sleep(Duration::from_millis(40)).await;
        assert!(!acquired.load(Ordering::SeqCst));

        drop(guard);
        tokio::time::timeout(Duration::from_secs(1), waiter)
            .await
            .expect("background task should complete after lock release")
            .expect("background task should not panic");

        assert!(acquired.load(Ordering::SeqCst));
    }

    #[tokio::test]
    #[serial]
    async fn map_sync_result_runs_error_handler_after_lock_release() {
        let result =
            run_with_sync_lock(async { Err::<(), AppError>(AppError::Config("boom".to_string())) })
                .await;

        let mut lock_released = false;
        let mapped = map_sync_result(result, |_| {
            lock_released = sync_mutex().try_lock().is_ok();
        });

        assert!(mapped.is_err());
        assert!(lock_released);
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
}
