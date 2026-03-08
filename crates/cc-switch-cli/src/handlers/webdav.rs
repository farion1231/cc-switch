//! WebDAV sync command handlers

use anyhow::anyhow;
use serde_json::{json, Value};

use crate::cli::WebDavCommands;
use crate::output::Printer;
use cc_switch_core::services::webdav_sync;
use cc_switch_core::{settings, AppError, AppState, ProviderService, WebDavSyncSettings};

pub async fn handle(
    cmd: WebDavCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        WebDavCommands::Show => handle_show(printer),
        WebDavCommands::Save {
            base_url,
            username,
            password,
            clear_password,
            remote_root,
            profile,
            enable,
            disable,
            auto_sync,
            no_auto_sync,
        } => {
            let settings = merge_settings(
                settings::get_webdav_sync_settings(),
                WebDavOverrides {
                    base_url,
                    username,
                    password,
                    clear_password,
                    remote_root,
                    profile,
                    enabled: resolve_toggle(enable, disable),
                    auto_sync: resolve_toggle(auto_sync, no_auto_sync),
                },
                true,
            )?;
            settings::set_webdav_sync_settings(Some(settings.clone()))?;
            printer.print_value(&json!({
                "success": true,
                "settings": sanitized_settings(&settings),
            }))?;
            Ok(())
        }
        WebDavCommands::Test {
            base_url,
            username,
            password,
            clear_password,
            remote_root,
            profile,
        } => {
            let settings = merge_settings(
                settings::get_webdav_sync_settings(),
                WebDavOverrides {
                    base_url,
                    username,
                    password,
                    clear_password,
                    remote_root,
                    profile,
                    enabled: None,
                    auto_sync: None,
                },
                false,
            )?;
            webdav_sync::check_connection(&settings).await?;
            printer.print_value(&json!({
                "success": true,
                "message": "WebDAV connection ok",
            }))?;
            Ok(())
        }
        WebDavCommands::Upload => {
            let mut settings = require_enabled_settings()?;
            let result =
                webdav_sync::run_with_sync_lock(webdav_sync::upload(&state.db, &mut settings))
                    .await;
            match result {
                Ok(value) => {
                    printer.print_value(&value)?;
                    Ok(())
                }
                Err(err) => {
                    persist_sync_error(&mut settings, &err, "manual");
                    Err(anyhow!(err.to_string()))
                }
            }
        }
        WebDavCommands::Download => {
            let mut sync_settings = require_enabled_settings()?;
            let result = webdav_sync::run_with_sync_lock(webdav_sync::download(
                &state.db,
                &mut sync_settings,
            ))
            .await;
            let mut value = match result {
                Ok(value) => value,
                Err(err) => {
                    persist_sync_error(&mut sync_settings, &err, "manual");
                    return Err(anyhow!(err.to_string()));
                }
            };

            let warning = run_post_download_sync(state).err().map(post_sync_warning);
            if let Some(message) = warning.as_ref() {
                printer.warn(message);
            }
            attach_warning(&mut value, warning);
            printer.print_value(&value)?;
            Ok(())
        }
        WebDavCommands::RemoteInfo => {
            let sync_settings = require_enabled_settings()?;
            let info = webdav_sync::fetch_remote_info(&sync_settings).await?;
            printer.print_value(&info.unwrap_or_else(|| json!({ "empty": true })))?;
            Ok(())
        }
    }
}

#[derive(Default)]
struct WebDavOverrides {
    base_url: Option<String>,
    username: Option<String>,
    password: Option<String>,
    clear_password: bool,
    remote_root: Option<String>,
    profile: Option<String>,
    enabled: Option<bool>,
    auto_sync: Option<bool>,
}

fn handle_show(printer: &Printer) -> anyhow::Result<()> {
    let payload = if let Some(settings) = settings::get_webdav_sync_settings() {
        json!({
            "configured": true,
            "settings": sanitized_settings(&settings),
        })
    } else {
        json!({ "configured": false })
    };
    printer.print_value(&payload)?;
    Ok(())
}

fn merge_settings(
    existing: Option<WebDavSyncSettings>,
    overrides: WebDavOverrides,
    default_enabled_on_new_save: bool,
) -> anyhow::Result<WebDavSyncSettings> {
    let mut settings = existing.clone().unwrap_or_default();

    if existing.is_none() && default_enabled_on_new_save {
        settings.enabled = true;
    }

    if let Some(base_url) = overrides.base_url {
        settings.base_url = base_url;
    }
    if let Some(username) = overrides.username {
        settings.username = username;
    }
    if let Some(remote_root) = overrides.remote_root {
        settings.remote_root = remote_root;
    }
    if let Some(profile) = overrides.profile {
        settings.profile = profile;
    }
    if let Some(enabled) = overrides.enabled {
        settings.enabled = enabled;
    }
    if let Some(auto_sync) = overrides.auto_sync {
        settings.auto_sync = auto_sync;
    }
    if overrides.clear_password {
        settings.password.clear();
    } else if let Some(password) = overrides.password {
        settings.password = password;
    }

    settings.normalize();
    settings.validate()?;
    Ok(settings)
}

fn resolve_toggle(enable: bool, disable: bool) -> Option<bool> {
    if enable {
        Some(true)
    } else if disable {
        Some(false)
    } else {
        None
    }
}

fn require_enabled_settings() -> anyhow::Result<WebDavSyncSettings> {
    let settings = settings::get_webdav_sync_settings()
        .ok_or_else(|| anyhow!(webdav_not_configured_error().to_string()))?;
    if !settings.enabled {
        return Err(anyhow!(webdav_disabled_error().to_string()));
    }
    Ok(settings)
}

fn sanitized_settings(settings: &WebDavSyncSettings) -> Value {
    json!({
        "enabled": settings.enabled,
        "autoSync": settings.auto_sync,
        "baseUrl": settings.base_url,
        "username": settings.username,
        "remoteRoot": settings.remote_root,
        "profile": settings.profile,
        "status": settings.status,
        "passwordConfigured": !settings.password.is_empty(),
    })
}

fn persist_sync_error(settings: &mut WebDavSyncSettings, error: &AppError, source: &str) {
    settings.status.last_error = Some(error.to_string());
    settings.status.last_error_source = Some(source.to_string());
    let _ = cc_switch_core::settings::update_webdav_sync_status(settings.status.clone());
}

fn run_post_download_sync(state: &AppState) -> Result<(), AppError> {
    ProviderService::sync_current_to_live(state)?;
    settings::reload_settings()
}

fn post_sync_warning(error: AppError) -> String {
    AppError::localized(
        "sync.post_operation_sync_failed",
        format!("后置同步状态失败: {error}"),
        format!("Post-operation synchronization failed: {error}"),
    )
    .to_string()
}

fn attach_warning(value: &mut Value, warning: Option<String>) {
    if let Some(message) = warning {
        if let Some(obj) = value.as_object_mut() {
            obj.insert("warning".to_string(), Value::String(message));
        }
    }
}

fn webdav_not_configured_error() -> AppError {
    AppError::localized(
        "webdav.sync.not_configured",
        "未配置 WebDAV 同步",
        "WebDAV sync is not configured.",
    )
}

fn webdav_disabled_error() -> AppError {
    AppError::localized(
        "webdav.sync.disabled",
        "WebDAV 同步未启用",
        "WebDAV sync is disabled.",
    )
}
