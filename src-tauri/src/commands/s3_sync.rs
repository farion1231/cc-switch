#![allow(non_snake_case)]

use serde_json::{json, Value};
use tauri::State;

use crate::commands::sync_support::{
    attach_warning, post_sync_warning_from_result, run_post_import_sync,
};
use crate::error::AppError;
use crate::services::s3_sync as s3_sync_service;
use crate::settings::{self, S3SyncSettings};
use crate::store::AppState;

fn persist_sync_error(settings: &mut S3SyncSettings, error: &AppError, source: &str) {
    settings.status.last_error = Some(error.to_string());
    settings.status.last_error_source = Some(source.to_string());
    let _ = settings::update_s3_sync_status(settings.status.clone());
}

fn s3_not_configured_error() -> String {
    AppError::localized(
        "s3.sync.not_configured",
        "未配置 S3 同步",
        "S3 sync is not configured.",
    )
    .to_string()
}

fn s3_sync_disabled_error() -> String {
    AppError::localized(
        "s3.sync.disabled",
        "S3 同步未启用",
        "S3 sync is disabled.",
    )
    .to_string()
}

fn require_enabled_s3_settings() -> Result<S3SyncSettings, String> {
    let settings = settings::get_s3_sync_settings().ok_or_else(s3_not_configured_error)?;
    if !settings.enabled {
        return Err(s3_sync_disabled_error());
    }
    Ok(settings)
}

fn resolve_secret_for_request(
    mut incoming: S3SyncSettings,
    existing: Option<S3SyncSettings>,
    preserve_empty_secret: bool,
) -> S3SyncSettings {
    if let Some(existing_settings) = existing {
        if preserve_empty_secret && incoming.secret_access_key.is_empty() {
            incoming.secret_access_key = existing_settings.secret_access_key;
        }
    }
    incoming
}

#[cfg(test)]
fn s3_sync_mutex() -> &'static tokio::sync::Mutex<()> {
    s3_sync_service::sync_mutex()
}

async fn run_with_s3_lock<T, Fut>(operation: Fut) -> Result<T, AppError>
where
    Fut: std::future::Future<Output = Result<T, AppError>>,
{
    s3_sync_service::run_with_sync_lock(operation).await
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

#[tauri::command]
pub async fn s3_test_connection(
    settings: S3SyncSettings,
    #[allow(non_snake_case)] preserveEmptyPassword: Option<bool>,
) -> Result<Value, String> {
    let preserve_empty = preserveEmptyPassword.unwrap_or(true);
    let resolved = resolve_secret_for_request(
        settings,
        settings::get_s3_sync_settings(),
        preserve_empty,
    );
    s3_sync_service::check_connection(&resolved)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "success": true,
        "message": "S3 connection ok"
    }))
}

#[tauri::command]
pub async fn s3_sync_upload(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    let mut settings = require_enabled_s3_settings()?;

    let result = run_with_s3_lock(s3_sync_service::upload(&db, &mut settings)).await;
    map_sync_result(result, |error| {
        persist_sync_error(&mut settings, error, "manual")
    })
}

#[tauri::command]
pub async fn s3_sync_download(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    let db_for_sync = db.clone();
    let mut settings = require_enabled_s3_settings()?;
    let _auto_sync_suppression = crate::services::s3_auto_sync::AutoSyncSuppressionGuard::new();

    let sync_result = run_with_s3_lock(s3_sync_service::download(&db, &mut settings)).await;
    let mut result = map_sync_result(sync_result, |error| {
        persist_sync_error(&mut settings, error, "manual")
    })?;

    // Post-download sync is best-effort: snapshot restore has already succeeded.
    let warning = post_sync_warning_from_result(
        tauri::async_runtime::spawn_blocking(move || run_post_import_sync(db_for_sync))
            .await
            .map_err(|e| e.to_string()),
    );
    if let Some(msg) = warning.as_ref() {
        log::warn!("[S3] post-download sync warning: {msg}");
    }
    result = attach_warning(result, warning);

    Ok(result)
}

#[tauri::command]
pub async fn s3_sync_save_settings(
    settings: S3SyncSettings,
    #[allow(non_snake_case)] passwordTouched: Option<bool>,
) -> Result<Value, String> {
    let password_touched = passwordTouched.unwrap_or(false);
    let existing = settings::get_s3_sync_settings();
    let mut sync_settings =
        resolve_secret_for_request(settings, existing.clone(), !password_touched);

    // Preserve server-owned fields that the frontend does not manage
    if let Some(existing_settings) = existing {
        sync_settings.status = existing_settings.status;
    }

    sync_settings.normalize();
    sync_settings.validate().map_err(|e| e.to_string())?;
    settings::set_s3_sync_settings(Some(sync_settings)).map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn s3_sync_fetch_remote_info() -> Result<Value, String> {
    let settings = require_enabled_s3_settings()?;
    let info = s3_sync_service::fetch_remote_info(&settings)
        .await
        .map_err(|e| e.to_string())?;
    Ok(info.unwrap_or(json!({ "empty": true })))
}

#[cfg(test)]
mod tests {
    use super::{
        map_sync_result, persist_sync_error, require_enabled_s3_settings,
        resolve_secret_for_request, run_with_s3_lock, s3_sync_mutex,
    };
    use crate::error::AppError;
    use crate::settings::{AppSettings, S3SyncSettings};
    use serial_test::serial;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn s3_sync_mutex_is_singleton() {
        let a = s3_sync_mutex() as *const _;
        let b = s3_sync_mutex() as *const _;
        assert_eq!(a, b);
    }

    #[tokio::test]
    #[serial]
    async fn s3_sync_mutex_serializes_concurrent_access() {
        let guard = s3_sync_mutex().lock().await;
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_bg = Arc::clone(&acquired);

        let waiter = tokio::spawn(async move {
            let _inner_guard = s3_sync_mutex().lock().await;
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
        let result = run_with_s3_lock(async {
            Err::<(), AppError>(AppError::Config("boom".to_string()))
        })
        .await;

        let mut lock_released = false;
        let mapped = map_sync_result(result, |_| {
            lock_released = s3_sync_mutex().try_lock().is_ok();
        });

        assert!(mapped.is_err());
        assert!(lock_released);
    }

    #[test]
    fn resolve_secret_for_request_preserves_existing_when_requested() {
        let incoming = S3SyncSettings {
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: String::new(),
            ..S3SyncSettings::default()
        };
        let existing = Some(S3SyncSettings {
            secret_access_key: "SECRET".to_string(),
            ..S3SyncSettings::default()
        });
        let resolved = resolve_secret_for_request(incoming, existing, true);
        assert_eq!(resolved.secret_access_key, "SECRET");
    }

    #[test]
    fn resolve_secret_for_request_allows_explicit_empty_secret() {
        let incoming = S3SyncSettings {
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: String::new(),
            ..S3SyncSettings::default()
        };
        let existing = Some(S3SyncSettings {
            secret_access_key: "SECRET".to_string(),
            ..S3SyncSettings::default()
        });
        let resolved = resolve_secret_for_request(incoming, existing, false);
        assert!(resolved.secret_access_key.is_empty());
    }

    #[test]
    #[serial]
    fn persist_sync_error_updates_status_without_overwriting_credentials() {
        let test_home = std::env::temp_dir().join("cc-switch-s3-sync-error-status-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        crate::settings::update_settings(AppSettings::default()).expect("reset settings");
        let mut current = S3SyncSettings {
            enabled: true,
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..S3SyncSettings::default()
        };
        crate::settings::set_s3_sync_settings(Some(current.clone()))
            .expect("seed s3 settings");

        persist_sync_error(
            &mut current,
            &crate::error::AppError::Config("boom".to_string()),
            "manual",
        );

        let after = crate::settings::get_s3_sync_settings().expect("read s3 settings");
        assert_eq!(after.region, "us-east-1");
        assert_eq!(after.bucket, "my-bucket");
        assert_eq!(after.access_key_id, "AKID");
        assert_eq!(after.secret_access_key, "SECRET");
        assert_eq!(after.remote_root, "cc-switch-sync");
        assert_eq!(after.profile, "default");
        assert!(
            after
                .status
                .last_error
                .as_deref()
                .unwrap_or_default()
                .contains("boom"),
            "status error should be updated"
        );
        assert_eq!(after.status.last_error_source.as_deref(), Some("manual"));
    }

    #[test]
    #[serial]
    fn require_enabled_s3_settings_rejects_disabled_config() {
        let test_home = std::env::temp_dir().join("cc-switch-s3-sync-enabled-disabled-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        crate::settings::update_settings(AppSettings::default()).expect("reset settings");
        crate::settings::set_s3_sync_settings(Some(S3SyncSettings {
            enabled: false,
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            ..S3SyncSettings::default()
        }))
        .expect("seed disabled s3 settings");

        let err = require_enabled_s3_settings().expect_err("disabled settings should fail");
        assert!(
            err.contains("disabled") || err.contains("未启用"),
            "unexpected error: {err}"
        );
    }

    #[test]
    #[serial]
    fn require_enabled_s3_settings_returns_settings_when_enabled() {
        let test_home = std::env::temp_dir().join("cc-switch-s3-sync-enabled-ok-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        crate::settings::update_settings(AppSettings::default()).expect("reset settings");
        crate::settings::set_s3_sync_settings(Some(S3SyncSettings {
            enabled: true,
            region: "us-east-1".to_string(),
            bucket: "my-bucket".to_string(),
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            ..S3SyncSettings::default()
        }))
        .expect("seed enabled s3 settings");

        let settings =
            require_enabled_s3_settings().expect("enabled settings should be accepted");
        assert!(settings.enabled);
        assert_eq!(settings.region, "us-east-1");
    }
}
