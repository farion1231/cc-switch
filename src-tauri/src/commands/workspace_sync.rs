#![allow(non_snake_case)]

//! Tauri commands for workspace data sync (session/task/plan/memory).
//!
//! Credentials are reused from the existing WebDAV/S3 sync settings; the
//! workspace settings only pick the transport, providers, and remote root.

use serde_json::{json, Value};

use crate::error::AppError;
use crate::settings::{self, WorkspaceSyncSettings};
use crate::workspace_sync::adapters::adapter_for;
use crate::workspace_sync::engine;
use crate::workspace_sync::model::WorkspaceProviderId;
use crate::workspace_sync::storage::{s3::S3ObjectStorage, webdav::WebDavObjectStorage, ObjectStorage};

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

fn not_configured() -> String {
    AppError::localized(
        "workspace.sync.not_configured",
        "未配置工作区同步",
        "Workspace sync is not configured.",
    )
    .to_string()
}

fn disabled() -> String {
    AppError::localized(
        "workspace.sync.disabled",
        "工作区同步未启用",
        "Workspace sync is disabled.",
    )
    .to_string()
}

fn require_enabled_settings() -> Result<WorkspaceSyncSettings, String> {
    let settings = settings::get_workspace_sync_settings().ok_or_else(not_configured)?;
    if !settings.enabled {
        return Err(disabled());
    }
    Ok(settings)
}

/// Build the ObjectStorage for the configured transport, reusing existing creds.
fn resolve_storage(settings: &WorkspaceSyncSettings) -> Result<Box<dyn ObjectStorage>, String> {
    match settings.transport.as_str() {
        "s3" => {
            let s3 = settings::get_s3_sync_settings().ok_or_else(|| {
                AppError::localized(
                    "workspace.sync.s3_not_configured",
                    "工作区同步选择了 S3，但未配置 S3 凭据",
                    "Workspace sync selected S3 but S3 credentials are not configured.",
                )
                .to_string()
            })?;
            Ok(Box::new(S3ObjectStorage::from_settings(&s3)))
        }
        _ => {
            let webdav = settings::get_webdav_sync_settings().ok_or_else(|| {
                AppError::localized(
                    "workspace.sync.webdav_not_configured",
                    "工作区同步选择了 WebDAV，但未配置 WebDAV 凭据",
                    "Workspace sync selected WebDAV but WebDAV credentials are not configured.",
                )
                .to_string()
            })?;
            Ok(Box::new(WebDavObjectStorage::from_settings(&webdav)))
        }
    }
}

fn persist_status_error(settings: &mut WorkspaceSyncSettings, error: &str, source: &str) {
    settings.status.last_error = Some(error.to_string());
    settings.status.last_error_source = Some(source.to_string());
    let _ = settings::update_workspace_sync_status(settings.status.clone());
}

fn persist_status_success(settings: &mut WorkspaceSyncSettings) {
    settings.status.last_error = None;
    settings.status.last_error_source = None;
    settings.status.last_sync_at = Some(chrono::Utc::now().timestamp());
    let _ = settings::update_workspace_sync_status(settings.status.clone());
}

// ─── Commands ────────────────────────────────────────────────

/// Back up selected providers' local data to the cloud.
#[tauri::command]
pub async fn workspace_sync_backup() -> Result<Value, String> {
    let mut settings = require_enabled_settings()?;
    let storage = resolve_storage(&settings)?;

    let result = engine::run_with_sync_lock(engine::backup(&*storage, &settings, now_ms())).await;
    match result {
        Ok(report) => {
            persist_status_success(&mut settings);
            Ok(serde_json::to_value(report).unwrap_or_else(|_| json!({ "status": "ok" })))
        }
        Err(e) => {
            persist_status_error(&mut settings, &e.to_string(), "backup");
            Err(e.to_string())
        }
    }
}

/// Pull remote data, merge with local (union + keep-both), then back up the
/// merged result.
#[tauri::command]
pub async fn workspace_sync_merge() -> Result<Value, String> {
    let mut settings = require_enabled_settings()?;
    let storage = resolve_storage(&settings)?;

    let result = engine::run_with_sync_lock(engine::merge(&*storage, &settings, now_ms())).await;
    match result {
        Ok(report) => {
            persist_status_success(&mut settings);
            Ok(serde_json::to_value(report).unwrap_or_else(|_| json!({ "status": "ok" })))
        }
        Err(e) => {
            persist_status_error(&mut settings, &e.to_string(), "merge");
            Err(e.to_string())
        }
    }
}

/// Preview: per-provider installed flag and local item count (no network).
#[tauri::command]
pub async fn workspace_sync_scan_preview() -> Result<Value, String> {
    let requested = settings::get_workspace_sync_settings()
        .map(|s| s.providers)
        .unwrap_or_default();
    // If nothing selected yet, preview all supported providers.
    let providers: Vec<WorkspaceProviderId> = if requested.is_empty() {
        WorkspaceProviderId::all().to_vec()
    } else {
        requested
            .iter()
            .filter_map(|p| WorkspaceProviderId::parse(p))
            .collect()
    };

    let mut out = Vec::new();
    for provider in providers {
        let adapter = adapter_for(provider);
        let installed = adapter.is_installed();
        let count = if installed {
            adapter.scan().map(|v| v.len()).unwrap_or(0)
        } else {
            0
        };
        out.push(json!({
            "provider": provider.as_str(),
            "installed": installed,
            "itemCount": count,
        }));
    }
    Ok(json!({ "providers": out }))
}

/// Fetch remote head info without downloading blobs.
#[tauri::command]
pub async fn workspace_sync_fetch_remote_info() -> Result<Value, String> {
    let settings = require_enabled_settings()?;
    let storage = resolve_storage(&settings)?;
    let prefix = format!(
        "{}/v1/{}",
        settings.remote_root.trim_end_matches('/'),
        settings.profile
    );
    let head_key = format!("{prefix}/head.json");
    match storage.get(&head_key).await.map_err(|e| e.to_string())? {
        Some(obj) => {
            let head: engine::Head = serde_json::from_slice(&obj.bytes)
                .map_err(|e| format!("failed to parse remote head: {e}"))?;
            Ok(serde_json::to_value(head).unwrap_or(json!({ "empty": true })))
        }
        None => Ok(json!({ "empty": true })),
    }
}

/// Save workspace sync settings (normalized).
#[tauri::command]
pub async fn workspace_sync_save_settings(
    settings: WorkspaceSyncSettings,
) -> Result<Value, String> {
    let mut next = settings;
    // Preserve server-owned status across saves.
    if let Some(existing) = settings::get_workspace_sync_settings() {
        next.status = existing.status;
    }
    next.normalize();
    settings::set_workspace_sync_settings(Some(next)).map_err(|e| e.to_string())?;
    Ok(json!({ "success": true }))
}

/// Load workspace sync settings (for the UI to hydrate its form).
#[tauri::command]
pub async fn workspace_sync_get_settings() -> Result<Value, String> {
    let settings = settings::get_workspace_sync_settings().unwrap_or_default();
    Ok(serde_json::to_value(settings).unwrap_or(json!({})))
}
