#![allow(non_snake_case)]

//! Tauri commands for workspace data sync (session/task/plan/memory).
//!
//! Credentials are reused from the existing WebDAV/S3 sync settings; the
//! workspace settings only pick the transport, providers, and remote root.

use std::sync::Arc;

use serde_json::{json, Value};

use crate::database::Database;
use crate::error::AppError;
use crate::settings::{self, WorkspaceSyncSettings};
use crate::store::AppState;
use crate::workspace_sync::adapters::adapter_for;
use crate::workspace_sync::{config_sync, engine};
use crate::workspace_sync::model::WorkspaceProviderId;
use crate::workspace_sync::storage::{s3::S3ObjectStorage, webdav::WebDavObjectStorage, ObjectStorage};
use tauri::State;

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

/// Resolved transport: the object storage plus the remote root & profile taken
/// from the selected WebDAV/S3 settings (so workspace + config sync share one
/// remote location instead of a separate `cc-switch-workspace` root).
pub struct ResolvedTransport {
    pub storage: Box<dyn ObjectStorage>,
    pub remote_root: String,
    pub profile: String,
}

/// Build the ObjectStorage for the configured transport, reusing existing creds,
/// and derive the shared remote root/profile from those same settings.
fn resolve_storage(settings: &WorkspaceSyncSettings) -> Result<ResolvedTransport, String> {
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
            Ok(ResolvedTransport {
                remote_root: s3.remote_root.trim_end_matches('/').to_string(),
                profile: s3.profile.clone(),
                storage: Box::new(S3ObjectStorage::from_settings(&s3)),
            })
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
            Ok(ResolvedTransport {
                remote_root: webdav.remote_root.trim_end_matches('/').to_string(),
                profile: webdav.profile.clone(),
                storage: Box::new(WebDavObjectStorage::from_settings(&webdav)),
            })
        }
    }
}

/// Overlay the resolved (shared) remote root/profile onto a workspace settings
/// copy so the engine writes under the same location as config sync.
fn with_shared_location(
    mut settings: WorkspaceSyncSettings,
    transport: &ResolvedTransport,
) -> WorkspaceSyncSettings {
    settings.remote_root = transport.remote_root.clone();
    settings.profile = transport.profile.clone();
    settings
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

/// Run the unified sync (workspace data + config DB) under one lock. Shared by
/// the manual command and the scheduled auto-sync timer.
///
/// `source` labels persisted errors ("manual" | "auto"). When `force` is false
/// (scheduled ticks), a cheap local fingerprint + config-hash check short-
/// circuits the network round-trip if nothing changed locally since last sync.
/// Returns a JSON report.
pub async fn run_unified_sync(
    db: Arc<Database>,
    source: &str,
    force: bool,
) -> Result<Value, AppError> {
    let settings = settings::get_workspace_sync_settings()
        .ok_or_else(|| AppError::Message(not_configured()))?;
    if !settings.enabled {
        return Err(AppError::Message(disabled()));
    }
    let transport = resolve_storage(&settings).map_err(AppError::Message)?;
    let effective = with_shared_location(settings.clone(), &transport);

    // Skip check: if nothing changed locally (data fingerprint matches AND the
    // config DB snapshot hash matches), a scheduled tick does no network I/O.
    let local_fp = engine::compute_local_fingerprint(&effective);
    if !force {
        let data_unchanged = settings
            .status
            .last_local_fingerprint
            .as_deref()
            .map(|h| h == local_fp)
            .unwrap_or(false);
        let config_unchanged = config_sync::local_config_unchanged(&db, &settings.status);
        if data_unchanged && config_unchanged {
            return Ok(json!({ "status": "skipped", "reason": "no local changes" }));
        }
    }

    let db_for_lock = db.clone();
    let fp_for_lock = local_fp.clone();
    let result: Result<Value, AppError> = engine::run_with_sync_lock(async move {
        // 1) Workspace data sync (sessions/plans/memory/indexes/sqlite).
        let report = engine::sync(&*transport.storage, &effective, now_ms()).await?;

        // 2) Config-DB sync (whole-DB newer-wins) under the same lock.
        let status = settings.status.clone();
        let mut status = status;
        let config = config_sync::sync_config(
            &*transport.storage,
            &db_for_lock,
            &transport.remote_root,
            &transport.profile,
            &mut status,
        )
        .await?;
        // Record the local data fingerprint so the next tick can skip.
        status.last_local_fingerprint = Some(fp_for_lock);
        // Persist the updated markers immediately (still under the lock).
        let _ = settings::update_workspace_sync_status(status);

        let mut value =
            serde_json::to_value(&report).unwrap_or_else(|_| json!({ "status": "ok" }));
        if let Some(obj) = value.as_object_mut() {
            obj.insert(
                "config".to_string(),
                json!({
                    "action": config.action,
                    "syncedAt": config.synced_at,
                }),
            );
        }
        Ok(value)
    })
    .await;

    // Reload settings (the config phase may have imported a new DB → new
    // settings row); persist success/error markers onto the current settings.
    let mut settings = settings::get_workspace_sync_settings().unwrap_or_default();
    match &result {
        Ok(_) => persist_status_success(&mut settings),
        Err(e) => persist_status_error(&mut settings, &e.to_string(), source),
    }
    result
}

/// The one-button sync: workspace data + config DB in a single round.
#[tauri::command]
pub async fn workspace_sync_run(state: State<'_, AppState>) -> Result<Value, String> {
    run_unified_sync(state.db.clone(), "manual", true)
        .await
        .map_err(|e| e.to_string())
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

/// Fetch remote archive info (presence + snapshot id) without deploying.
#[tauri::command]
pub async fn workspace_sync_fetch_remote_info() -> Result<Value, String> {
    let settings = require_enabled_settings()?;
    let transport = resolve_storage(&settings)?;
    let prefix = format!(
        "{}/v1/{}",
        transport.remote_root.trim_end_matches('/'),
        transport.profile
    );
    let archive_key = format!("{prefix}/workspace.zip");
    match transport
        .storage
        .get(&archive_key)
        .await
        .map_err(|e| e.to_string())?
    {
        Some(obj) => {
            let archive = crate::workspace_sync::archive::unpack_snapshot(&obj.bytes)
                .map_err(|e| format!("failed to parse remote archive: {e}"))?;
            let m = archive.manifest();
            Ok(json!({
                "snapshotId": m.snapshot_id,
                "deviceName": m.created_by,
                "updatedAt": m.created_at,
                "sizeBytes": obj.bytes.len(),
            }))
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
