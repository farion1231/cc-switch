#![allow(non_snake_case)]

use serde_json::{Value, json};
use std::future::Future;
use std::sync::OnceLock;
use tauri::State;

use crate::commands::sync_support::{
    attach_warning, post_sync_warning_from_result, run_post_import_sync,
};
use crate::error::AppError;
use crate::services::webdav_sync as webdav_sync_service;
use crate::settings::{self, WebDavSyncSettings};
use crate::store::AppState;

fn persist_sync_error(settings: &mut WebDavSyncSettings, error: &AppError) {
    settings.status.last_error = Some(error.to_string());
    let _ = settings::update_webdav_sync_status(settings.status.clone());
}

fn webdav_not_configured_error() -> String {
    AppError::localized(
        "webdav.sync.not_configured",
        "未配置 WebDAV 同步",
        "WebDAV sync is not configured.",
    )
    .to_string()
}

fn webdav_sync_disabled_error() -> String {
    AppError::localized(
        "webdav.sync.disabled",
        "WebDAV 同步未启用",
        "WebDAV sync is disabled.",
    )
    .to_string()
}

fn require_enabled_webdav_settings() -> Result<WebDavSyncSettings, String> {
    let settings = settings::get_webdav_sync_settings().ok_or_else(webdav_not_configured_error)?;
    if !settings.enabled {
        return Err(webdav_sync_disabled_error());
    }
    Ok(settings)
}

fn resolve_password_for_request(
    mut incoming: WebDavSyncSettings,
    existing: Option<WebDavSyncSettings>,
    preserve_empty_password: bool,
) -> WebDavSyncSettings {
    if let Some(existing_settings) = existing {
        if preserve_empty_password && incoming.password.is_empty() {
            incoming.password = existing_settings.password;
        }
    }
    incoming
}

fn webdav_sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

async fn run_with_webdav_lock<T, Fut>(operation: Fut) -> Result<T, AppError>
where
    Fut: Future<Output = Result<T, AppError>>,
{
    let result = {
        let _guard = webdav_sync_mutex().lock().await;
        operation.await
    };
    result
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
pub async fn webdav_test_connection(
    settings: WebDavSyncSettings,
    #[allow(non_snake_case)] preserveEmptyPassword: Option<bool>,
) -> Result<Value, String> {
    let preserve_empty = preserveEmptyPassword.unwrap_or(true);
    let resolved = resolve_password_for_request(
        settings,
        settings::get_webdav_sync_settings(),
        preserve_empty,
    );
    webdav_sync_service::check_connection(&resolved)
        .await
        .map_err(|e| e.to_string())?;
    Ok(json!({
        "success": true,
        "message": "WebDAV connection ok"
    }))
}

#[tauri::command]
pub async fn webdav_sync_upload(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    let mut settings = require_enabled_webdav_settings()?;

    let result = run_with_webdav_lock(webdav_sync_service::upload(&db, &mut settings)).await;
    map_sync_result(result, |error| persist_sync_error(&mut settings, error))
}

#[tauri::command]
pub async fn webdav_sync_download(state: State<'_, AppState>) -> Result<Value, String> {
    let db = state.db.clone();
    let db_for_sync = db.clone();
    let mut settings = require_enabled_webdav_settings()?;

    let sync_result = run_with_webdav_lock(webdav_sync_service::download(&db, &mut settings)).await;
    let mut result = map_sync_result(sync_result, |error| {
        persist_sync_error(&mut settings, error)
    })?;

    // Post-download sync is best-effort: snapshot restore has already succeeded.
    let warning = post_sync_warning_from_result(
        tauri::async_runtime::spawn_blocking(move || run_post_import_sync(db_for_sync))
            .await
            .map_err(|e| e.to_string()),
    );
    if let Some(msg) = warning.as_ref() {
        log::warn!("[WebDAV] post-download sync warning: {msg}");
    }
    result = attach_warning(result, warning);

    Ok(result)
}

#[tauri::command]
pub async fn webdav_sync_save_settings(
    settings: WebDavSyncSettings,
    #[allow(non_snake_case)] passwordTouched: Option<bool>,
) -> Result<Value, String> {
    let password_touched = passwordTouched.unwrap_or(false);
    let existing = settings::get_webdav_sync_settings();
    let mut sync_settings =
        resolve_password_for_request(settings, existing.clone(), !password_touched);

    // Preserve server-owned fields that the frontend does not manage
    if let Some(existing_settings) = existing {
        sync_settings.status = existing_settings.status;
    }

    sync_settings.normalize();
    sync_settings.validate().map_err(|e| e.to_string())?;
    settings::set_webdav_sync_settings(Some(sync_settings)).map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

#[tauri::command]
pub async fn webdav_sync_fetch_remote_info() -> Result<Value, String> {
    let settings = require_enabled_webdav_settings()?;
    let info = webdav_sync_service::fetch_remote_info(&settings)
        .await
        .map_err(|e| e.to_string())?;
    Ok(info.unwrap_or(json!({ "empty": true })))
}

#[cfg(test)]
mod tests {
    use super::{
        map_sync_result, persist_sync_error, require_enabled_webdav_settings,
        resolve_password_for_request, run_with_webdav_lock, webdav_sync_mutex,
    };
    use crate::error::AppError;
    use crate::settings::{AppSettings, WebDavSyncSettings};
    use serial_test::serial;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn webdav_sync_mutex_is_singleton() {
        let a = webdav_sync_mutex() as *const _;
        let b = webdav_sync_mutex() as *const _;
        assert_eq!(a, b);
    }

    #[tokio::test]
    #[serial]
    async fn webdav_sync_mutex_serializes_concurrent_access() {
        let guard = webdav_sync_mutex().lock().await;
        let acquired = Arc::new(AtomicBool::new(false));
        let acquired_bg = Arc::clone(&acquired);

        let waiter = tokio::spawn(async move {
            let _inner_guard = webdav_sync_mutex().lock().await;
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
        let result = run_with_webdav_lock(async {
            Err::<(), AppError>(AppError::Config("boom".to_string()))
        })
        .await;

        let mut lock_released = false;
        let mapped = map_sync_result(result, |_| {
            lock_released = webdav_sync_mutex().try_lock().is_ok();
        });

        assert!(mapped.is_err());
        assert!(lock_released);
    }

    #[test]
    fn resolve_password_for_request_preserves_existing_when_requested() {
        let incoming = WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: String::new(),
            ..WebDavSyncSettings::default()
        };
        let existing = Some(WebDavSyncSettings {
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        });
        let resolved = resolve_password_for_request(incoming, existing, true);
        assert_eq!(resolved.password, "secret");
    }

    #[test]
    fn resolve_password_for_request_allows_explicit_empty_password() {
        let incoming = WebDavSyncSettings {
            base_url: "https://dav.example.com".to_string(),
            username: "alice".to_string(),
            password: String::new(),
            ..WebDavSyncSettings::default()
        };
        let existing = Some(WebDavSyncSettings {
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        });
        let resolved = resolve_password_for_request(incoming, existing, false);
        assert!(resolved.password.is_empty());
    }

    #[test]
    #[serial]
    fn persist_sync_error_updates_status_without_overwriting_credentials() {
        let test_home = std::env::temp_dir().join("cc-switch-sync-error-status-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        crate::settings::update_settings(AppSettings::default()).expect("reset settings");
        let mut current = WebDavSyncSettings {
            enabled: true,
            base_url: "https://dav.example.com/dav/".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        crate::settings::set_webdav_sync_settings(Some(current.clone()))
            .expect("seed webdav settings");

        persist_sync_error(
            &mut current,
            &crate::error::AppError::Config("boom".to_string()),
        );

        let after = crate::settings::get_webdav_sync_settings().expect("read webdav settings");
        assert_eq!(after.base_url, "https://dav.example.com/dav/");
        assert_eq!(after.username, "alice");
        assert_eq!(after.password, "secret");
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
    }

    #[test]
    #[serial]
    fn require_enabled_webdav_settings_rejects_disabled_config() {
        let test_home = std::env::temp_dir().join("cc-switch-sync-enabled-disabled-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        crate::settings::update_settings(AppSettings::default()).expect("reset settings");
        crate::settings::set_webdav_sync_settings(Some(WebDavSyncSettings {
            enabled: false,
            base_url: "https://dav.example.com/dav/".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        }))
        .expect("seed disabled webdav settings");

        let err = require_enabled_webdav_settings().expect_err("disabled settings should fail");
        assert!(
            err.contains("disabled") || err.contains("未启用"),
            "unexpected error: {err}"
        );
    }

    #[test]
    #[serial]
    fn require_enabled_webdav_settings_returns_settings_when_enabled() {
        let test_home = std::env::temp_dir().join("cc-switch-sync-enabled-ok-test");
        let _ = std::fs::remove_dir_all(&test_home);
        std::fs::create_dir_all(&test_home).expect("create test home");
        std::env::set_var("CC_SWITCH_TEST_HOME", &test_home);

        crate::settings::update_settings(AppSettings::default()).expect("reset settings");
        crate::settings::set_webdav_sync_settings(Some(WebDavSyncSettings {
            enabled: true,
            base_url: "https://dav.example.com/dav/".to_string(),
            username: "alice".to_string(),
            password: "secret".to_string(),
            ..WebDavSyncSettings::default()
        }))
        .expect("seed enabled webdav settings");

        let settings =
            require_enabled_webdav_settings().expect("enabled settings should be accepted");
        assert!(settings.enabled);
        assert_eq!(settings.base_url, "https://dav.example.com/dav/");
    }
}
