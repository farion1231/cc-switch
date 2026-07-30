//! WebDAV v2 sync protocol layer with DB compatibility subdirectories.
//!
//! Implements manifest-based synchronization on top of the HTTP transport
//! primitives in [`super::webdav`]. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::OnceLock;

use chrono::Utc;
use serde_json::Value;

use crate::error::AppError;
use crate::services::webdav::{
    auth_from_credentials, build_remote_url, ensure_remote_directories, get_bytes, head_etag,
    path_segments, put_bytes, put_bytes_with_precondition, test_connection, PutPrecondition,
    WebDavAuth,
};
use crate::settings::{update_webdav_sync_status, WebDavSyncSettings, WebDavSyncStatus};

use super::sync_protocol::{
    apply_snapshot, assess_snapshot_sync_state, build_local_snapshot, effective_db_compat_version,
    localized, manual_resolution_required_error, persist_sync_success_best_effort, sha256_hex,
    validate_artifact_size_limit, validate_manifest_compat, verify_artifact, ArtifactMeta,
    LocalSnapshot, RemoteLayout, SnapshotSyncState, SyncManifest, DB_COMPAT_VERSION,
    MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES, PROTOCOL_VERSION, REMOTE_DB_SQL, REMOTE_MANIFEST,
    REMOTE_SKILLS_ZIP, SYNC_MANUAL_RESOLUTION_REQUIRED_KEY,
};

pub(crate) mod archive;

// ─── Sync lock ───────────────────────────────────────────────

pub fn sync_mutex() -> &'static tokio::sync::Mutex<()> {
    static LOCK: OnceLock<tokio::sync::Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| tokio::sync::Mutex::new(()))
}

pub async fn run_with_sync_lock<T, Fut>(operation: Fut) -> Result<T, AppError>
where
    Fut: Future<Output = Result<T, AppError>>,
{
    let _guard = sync_mutex().lock().await;
    operation.await
}

pub(crate) struct PreparedAutoSyncUpload {
    snapshot: LocalSnapshot,
    guard: AutoSyncUploadGuard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum AutoSyncUploadGuard {
    RemoteMissing,
    CurrentRemote {
        manifest_etag: String,
        db_etag: String,
        skills_etag: String,
    },
}

pub(crate) struct RemoteSnapshot {
    layout: RemoteLayout,
    manifest: SyncManifest,
    manifest_bytes: Vec<u8>,
    manifest_etag: Option<String>,
}
// ─── Public API ──────────────────────────────────────────────

/// Check WebDAV connectivity and ensure remote directory structure.
pub async fn check_connection(settings: &WebDavSyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    test_connection(&settings.base_url, &auth).await?;
    let dir_segs = remote_dir_segments(settings, RemoteLayout::Current);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;
    Ok(())
}

/// Prepare an automatic upload only when the three-way sync state is safe.
pub async fn prepare_auto_sync_snapshot(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Option<PreparedAutoSyncUpload>, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let remote_snapshot = find_remote_snapshot(settings, &auth).await?;
    let local_snapshot = build_local_snapshot(db)?;
    let remote_manifest_hash = remote_snapshot
        .as_ref()
        .map(|snapshot| sha256_hex(&snapshot.manifest_bytes));
    let state = assess_snapshot_sync_state(
        &settings.status,
        &local_snapshot.snapshot_id,
        remote_snapshot
            .as_ref()
            .map(|snapshot| snapshot.manifest.snapshot_id.as_str()),
        remote_manifest_hash.as_deref(),
    );

    match state {
        SnapshotSyncState::RemoteMissing => Ok(Some(PreparedAutoSyncUpload {
            snapshot: local_snapshot,
            guard: AutoSyncUploadGuard::RemoteMissing,
        })),
        SnapshotSyncState::LocalOnlyChanged => Ok(Some(PreparedAutoSyncUpload {
            snapshot: local_snapshot,
            guard: guard_for_auto_upload(settings, &auth, remote_snapshot.as_ref()).await?,
        })),
        SnapshotSyncState::InSync => {
            if let Some(remote_snapshot) = remote_snapshot {
                let manifest_hash = sha256_hex(&remote_snapshot.manifest_bytes);
                let _persisted = persist_sync_success_best_effort(
                    settings,
                    manifest_hash,
                    remote_snapshot.manifest.snapshot_id.clone(),
                    remote_snapshot.manifest_etag,
                    persist_sync_success,
                );
            }
            Ok(None)
        }
        SnapshotSyncState::RemoteOnlyChanged | SnapshotSyncState::Conflict => {
            Err(manual_resolution_required_error(state))
        }
    }
}

/// Upload local snapshot (db + skills) to remote.
pub async fn upload(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    let snapshot = build_local_snapshot(db)?;
    upload_snapshot(settings, snapshot).await
}

/// Upload a prebuilt local snapshot to remote.
pub async fn upload_snapshot(
    settings: &mut WebDavSyncSettings,
    snapshot: LocalSnapshot,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let dir_segs = remote_dir_segments(settings, RemoteLayout::Current);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;

    // Upload order: artifacts first, manifest last (best-effort consistency)
    let db_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_DB_SQL)?;
    put_bytes(&db_url, &auth, snapshot.db_sql, "application/sql").await?;

    let skills_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_SKILLS_ZIP)?;
    put_bytes(&skills_url, &auth, snapshot.skills_zip, "application/zip").await?;

    let manifest_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_MANIFEST)?;
    put_bytes(
        &manifest_url,
        &auth,
        snapshot.manifest_bytes,
        "application/json",
    )
    .await?;

    // Fetch etag (best-effort, don't fail the upload)
    let etag = match head_etag(&manifest_url, &auth).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[WebDAV] Failed to fetch ETag after upload: {e}");
            None
        }
    };

    let _persisted = persist_sync_success_best_effort(
        settings,
        snapshot.manifest_hash,
        snapshot.snapshot_id,
        etag,
        persist_sync_success,
    );
    Ok(serde_json::json!({ "status": "uploaded" }))
}

/// Upload a prebuilt automatic-sync snapshot with conditional writes.
///
/// Manual uploads intentionally overwrite remote data. Automatic uploads use
/// WebDAV preconditions so a remote change during the upload window is surfaced
/// as a manual-resolution conflict instead of silently overwriting data.
pub async fn upload_prepared_auto_sync_snapshot(
    settings: &mut WebDavSyncSettings,
    prepared: PreparedAutoSyncUpload,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let dir_segs = remote_dir_segments(settings, RemoteLayout::Current);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;

    let db_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_DB_SQL)?;
    let skills_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_SKILLS_ZIP)?;
    let manifest_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_MANIFEST)?;
    let PreparedAutoSyncUpload { snapshot, guard } = prepared;

    match guard {
        AutoSyncUploadGuard::RemoteMissing => {
            put_bytes_guarded(
                &db_url,
                &auth,
                snapshot.db_sql,
                "application/sql",
                PutPrecondition::IfNoneMatchAny,
            )
            .await?;
            put_bytes_guarded(
                &skills_url,
                &auth,
                snapshot.skills_zip,
                "application/zip",
                PutPrecondition::IfNoneMatchAny,
            )
            .await?;
            put_bytes_guarded(
                &manifest_url,
                &auth,
                snapshot.manifest_bytes,
                "application/json",
                PutPrecondition::IfNoneMatchAny,
            )
            .await?;
        }
        AutoSyncUploadGuard::CurrentRemote {
            manifest_etag,
            db_etag,
            skills_etag,
        } => {
            put_bytes_guarded(
                &db_url,
                &auth,
                snapshot.db_sql,
                "application/sql",
                PutPrecondition::IfMatch(db_etag),
            )
            .await?;
            put_bytes_guarded(
                &skills_url,
                &auth,
                snapshot.skills_zip,
                "application/zip",
                PutPrecondition::IfMatch(skills_etag),
            )
            .await?;
            put_bytes_guarded(
                &manifest_url,
                &auth,
                snapshot.manifest_bytes,
                "application/json",
                PutPrecondition::IfMatch(manifest_etag),
            )
            .await?;
        }
    }

    let etag = match head_etag(&manifest_url, &auth).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[WebDAV] Failed to fetch ETag after auto upload: {e}");
            None
        }
    };

    let _persisted = persist_sync_success_best_effort(
        settings,
        snapshot.manifest_hash,
        snapshot.snapshot_id,
        etag,
        persist_sync_success,
    );
    Ok(serde_json::json!({ "status": "uploaded" }))
}

async fn guard_for_auto_upload(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    remote_snapshot: Option<&RemoteSnapshot>,
) -> Result<AutoSyncUploadGuard, AppError> {
    let Some(manifest_etag) = current_manifest_etag_for_auto_upload(remote_snapshot)? else {
        return Ok(AutoSyncUploadGuard::RemoteMissing);
    };

    let db_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_DB_SQL)?;
    let skills_url = remote_file_url(settings, RemoteLayout::Current, REMOTE_SKILLS_ZIP)?;
    let db_etag = required_current_etag(&db_url, auth).await?;
    let skills_etag = required_current_etag(&skills_url, auth).await?;

    Ok(AutoSyncUploadGuard::CurrentRemote {
        manifest_etag,
        db_etag,
        skills_etag,
    })
}

fn current_manifest_etag_for_auto_upload(
    remote_snapshot: Option<&RemoteSnapshot>,
) -> Result<Option<String>, AppError> {
    let Some(remote_snapshot) = remote_snapshot else {
        return Ok(None);
    };
    if remote_snapshot.layout != RemoteLayout::Current {
        return Ok(None);
    }
    let Some(manifest_etag) = remote_snapshot.manifest_etag.clone() else {
        return Err(auto_sync_precondition_unavailable_error());
    };
    Ok(Some(manifest_etag))
}

async fn required_current_etag(url: &str, auth: &WebDavAuth) -> Result<String, AppError> {
    head_etag(url, auth)
        .await?
        .ok_or_else(auto_sync_precondition_unavailable_error)
}

async fn put_bytes_guarded(
    url: &str,
    auth: &WebDavAuth,
    bytes: Vec<u8>,
    content_type: &str,
    precondition: PutPrecondition,
) -> Result<(), AppError> {
    put_bytes_with_precondition(url, auth, bytes, content_type, precondition)
        .await
        .map_err(|err| {
            if err.localized_key() == Some("webdav.put_precondition_failed") {
                manual_resolution_required_error(SnapshotSyncState::RemoteOnlyChanged)
            } else {
                err
            }
        })
}

fn auto_sync_precondition_unavailable_error() -> AppError {
    localized(
        SYNC_MANUAL_RESOLUTION_REQUIRED_KEY,
        "WebDAV 服务未返回可用于安全自动上传的 ETag，自动同步已暂停。请手动选择同步方向。",
        "The WebDAV service did not return an ETag for safe automatic upload, so auto sync paused. Choose a sync direction manually.",
    )
}

/// Download remote snapshot and apply to local database + skills.
pub async fn download(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let snapshot = find_remote_snapshot(settings, &auth)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_empty",
                "远端没有可下载的同步数据",
                "No downloadable sync data found on the remote.",
            )
        })?;

    validate_manifest_compat(&snapshot.manifest, snapshot.layout)?;

    // Download and verify artifacts
    let db_sql = download_and_verify(
        settings,
        &auth,
        snapshot.layout,
        REMOTE_DB_SQL,
        &snapshot.manifest.artifacts,
    )
    .await?;
    let skills_zip = download_and_verify(
        settings,
        &auth,
        snapshot.layout,
        REMOTE_SKILLS_ZIP,
        &snapshot.manifest.artifacts,
    )
    .await?;

    // Apply snapshot
    apply_snapshot(db, &db_sql, &skills_zip)?;

    let manifest_hash = sha256_hex(&snapshot.manifest_bytes);
    let _persisted = persist_sync_success_best_effort(
        settings,
        manifest_hash,
        snapshot.manifest.snapshot_id.clone(),
        snapshot.manifest_etag,
        persist_sync_success,
    );
    Ok(serde_json::json!({
        "status": "downloaded",
        "sourceLayout": snapshot.layout.as_str(),
        "sourcePath": remote_dir_display(settings, snapshot.layout),
    }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &WebDavSyncSettings) -> Result<Option<Value>, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let Some(snapshot) = find_remote_snapshot(settings, &auth).await? else {
        return Ok(None);
    };
    let compatible = validate_manifest_compat(&snapshot.manifest, snapshot.layout).is_ok();
    let db_compat_version = effective_db_compat_version(&snapshot.manifest, snapshot.layout);

    let payload = serde_json::json!({
        "deviceName": snapshot.manifest.device_name,
        "createdAt": snapshot.manifest.created_at,
        "snapshotId": snapshot.manifest.snapshot_id,
        "version": snapshot.manifest.version,
        "protocolVersion": snapshot.manifest.version,
        "dbCompatVersion": db_compat_version,
        "compatible": compatible,
        "artifacts": snapshot.manifest.artifacts.keys().collect::<Vec<_>>(),
        "layout": snapshot.layout.as_str(),
        "remotePath": remote_dir_display(settings, snapshot.layout),
    });

    Ok(Some(payload))
}

// ─── Sync status persistence ─────────────────────────────────

fn persist_sync_success(
    settings: &mut WebDavSyncSettings,
    manifest_hash: String,
    snapshot_id: String,
    etag: Option<String>,
) -> Result<(), AppError> {
    let status = WebDavSyncStatus {
        last_sync_at: Some(Utc::now().timestamp()),
        last_error: None,
        last_error_key: None,
        last_error_source: None,
        last_local_manifest_hash: Some(manifest_hash.clone()),
        last_remote_manifest_hash: Some(manifest_hash),
        last_local_snapshot_id: Some(snapshot_id.clone()),
        last_remote_snapshot_id: Some(snapshot_id),
        last_remote_etag: etag,
    };
    settings.status = status.clone();
    update_webdav_sync_status(status)
}

async fn find_remote_snapshot(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
) -> Result<Option<RemoteSnapshot>, AppError> {
    if let Some(snapshot) = fetch_remote_snapshot(settings, auth, RemoteLayout::Current).await? {
        return Ok(Some(snapshot));
    }
    fetch_remote_snapshot(settings, auth, RemoteLayout::Legacy).await
}

async fn fetch_remote_snapshot(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    layout: RemoteLayout,
) -> Result<Option<RemoteSnapshot>, AppError> {
    let manifest_url = remote_file_url(settings, layout, REMOTE_MANIFEST)?;
    let Some((manifest_bytes, manifest_etag)) =
        get_bytes(&manifest_url, auth, MAX_MANIFEST_BYTES).await?
    else {
        return Ok(None);
    };

    let manifest: SyncManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;

    Ok(Some(RemoteSnapshot {
        layout,
        manifest,
        manifest_bytes,
        manifest_etag,
    }))
}
// ─── Download & verify ───────────────────────────────────────

async fn download_and_verify(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
    layout: RemoteLayout,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let meta = artifacts.get(artifact_name).ok_or_else(|| {
        localized(
            "webdav.sync.manifest_missing_artifact",
            format!("manifest 中缺少 artifact: {artifact_name}"),
            format!("Manifest missing artifact: {artifact_name}"),
        )
    })?;
    validate_artifact_size_limit(artifact_name, meta.size)?;

    let url = remote_file_url(settings, layout, artifact_name)?;
    let (bytes, _) = get_bytes(&url, auth, MAX_SYNC_ARTIFACT_BYTES as usize)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_missing_artifact",
                format!("远端缺少 artifact 文件: {artifact_name}"),
                format!("Remote artifact file missing: {artifact_name}"),
            )
        })?;

    verify_artifact(&bytes, artifact_name, meta)?;
    Ok(bytes)
}

// ─── Remote path helpers ─────────────────────────────────────

fn remote_dir_segments(settings: &WebDavSyncSettings, layout: RemoteLayout) -> Vec<String> {
    let mut segs = Vec::new();
    segs.extend(path_segments(&settings.remote_root).map(str::to_string));
    segs.push(format!("v{PROTOCOL_VERSION}"));
    if layout == RemoteLayout::Current {
        segs.push(format!("db-v{DB_COMPAT_VERSION}"));
    }
    segs.extend(path_segments(&settings.profile).map(str::to_string));
    segs
}

fn remote_file_url(
    settings: &WebDavSyncSettings,
    layout: RemoteLayout,
    file_name: &str,
) -> Result<String, AppError> {
    let mut segs = remote_dir_segments(settings, layout);
    segs.extend(path_segments(file_name).map(str::to_string));
    build_remote_url(&settings.base_url, &segs)
}

fn remote_dir_display(settings: &WebDavSyncSettings, layout: RemoteLayout) -> String {
    let segs = remote_dir_segments(settings, layout);
    format!("/{}", segs.join("/"))
}

fn auth_for(settings: &WebDavSyncSettings) -> WebDavAuth {
    auth_from_credentials(&settings.username, &settings.password)
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn remote_dir_segments_uses_current_layout() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings, RemoteLayout::Current);
        assert_eq!(segs, vec!["cc-switch-sync", "v2", "db-v6", "default"]);
    }

    #[test]
    fn remote_dir_segments_uses_legacy_layout() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings, RemoteLayout::Legacy);
        assert_eq!(segs, vec!["cc-switch-sync", "v2", "default"]);
    }

    fn remote_snapshot(layout: RemoteLayout, etag: Option<&str>) -> RemoteSnapshot {
        RemoteSnapshot {
            layout,
            manifest: SyncManifest {
                format: super::super::sync_protocol::PROTOCOL_FORMAT.to_string(),
                version: PROTOCOL_VERSION,
                db_compat_version: Some(DB_COMPAT_VERSION),
                device_name: "test-device".to_string(),
                created_at: "2026-06-10T00:00:00Z".to_string(),
                artifacts: BTreeMap::new(),
                snapshot_id: "base".to_string(),
            },
            manifest_bytes: Vec::new(),
            manifest_etag: etag.map(str::to_string),
        }
    }

    #[test]
    fn auto_upload_guard_treats_missing_remote_as_create_only() {
        assert_eq!(
            current_manifest_etag_for_auto_upload(None).expect("missing remote should be guarded"),
            None
        );
    }

    #[test]
    fn auto_upload_guard_treats_legacy_remote_as_current_create_only() {
        let remote = remote_snapshot(RemoteLayout::Legacy, Some("\"etag\""));
        assert_eq!(
            current_manifest_etag_for_auto_upload(Some(&remote))
                .expect("legacy remote should be guarded"),
            None
        );
    }

    #[test]
    fn auto_upload_guard_requires_current_manifest_etag() {
        let remote = remote_snapshot(RemoteLayout::Current, None);
        let err = current_manifest_etag_for_auto_upload(Some(&remote))
            .expect_err("missing etag should fail safe");
        assert_eq!(
            err.localized_key(),
            Some(SYNC_MANUAL_RESOLUTION_REQUIRED_KEY)
        );
    }

    #[test]
    fn auto_upload_guard_uses_current_manifest_etag() {
        let remote = remote_snapshot(RemoteLayout::Current, Some("\"etag\""));
        assert_eq!(
            current_manifest_etag_for_auto_upload(Some(&remote))
                .expect("etag should create if-match guard"),
            Some("\"etag\"".to_string())
        );
    }
}
