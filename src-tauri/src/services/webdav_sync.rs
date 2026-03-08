//! WebDAV v2 sync protocol layer.
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
    path_segments, put_bytes, test_connection, WebDavAuth,
};
use crate::settings::{update_webdav_sync_status, WebDavSyncSettings, WebDavSyncStatus};

use super::sync_protocol::{
    apply_snapshot, build_local_snapshot, localized, persist_sync_success_best_effort, sha256_hex,
    validate_artifact_size_limit, validate_manifest_compat, verify_artifact, ArtifactMeta,
    SyncManifest, MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES, PROTOCOL_VERSION, REMOTE_DB_SQL,
    REMOTE_MANIFEST, REMOTE_SKILLS_ZIP,
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

// ─── Public API ──────────────────────────────────────────────

/// Check WebDAV connectivity and ensure remote directory structure.
pub async fn check_connection(settings: &WebDavSyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    test_connection(&settings.base_url, &auth).await?;
    let dir_segs = remote_dir_segments(settings);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;
    Ok(())
}

/// Upload local snapshot (db + skills) to remote.
pub async fn upload(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let dir_segs = remote_dir_segments(settings);
    ensure_remote_directories(&settings.base_url, &dir_segs, &auth).await?;

    let snapshot = build_local_snapshot(db)?;

    // Upload order: artifacts first, manifest last (best-effort consistency)
    let db_url = remote_file_url(settings, REMOTE_DB_SQL)?;
    put_bytes(&db_url, &auth, snapshot.db_sql, "application/sql").await?;

    let skills_url = remote_file_url(settings, REMOTE_SKILLS_ZIP)?;
    put_bytes(&skills_url, &auth, snapshot.skills_zip, "application/zip").await?;

    let manifest_url = remote_file_url(settings, REMOTE_MANIFEST)?;
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
        etag,
        persist_sync_success,
    );
    Ok(serde_json::json!({ "status": "uploaded" }))
}

/// Download remote snapshot and apply to local database + skills.
pub async fn download(
    db: &crate::database::Database,
    settings: &mut WebDavSyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);

    let manifest_url = remote_file_url(settings, REMOTE_MANIFEST)?;
    let (manifest_bytes, etag) = get_bytes(&manifest_url, &auth, MAX_MANIFEST_BYTES)
        .await?
        .ok_or_else(|| {
            localized(
                "webdav.sync.remote_empty",
                "远端没有可下载的同步数据",
                "No downloadable sync data found on the remote.",
            )
        })?;

    let manifest: SyncManifest =
        serde_json::from_slice(&manifest_bytes).map_err(|e| AppError::Json {
            path: REMOTE_MANIFEST.to_string(),
            source: e,
        })?;

    validate_manifest_compat(&manifest)?;

    // Download and verify artifacts
    let db_sql = download_and_verify(settings, &auth, REMOTE_DB_SQL, &manifest.artifacts).await?;
    let skills_zip =
        download_and_verify(settings, &auth, REMOTE_SKILLS_ZIP, &manifest.artifacts).await?;

    // Apply snapshot
    apply_snapshot(db, &db_sql, &skills_zip)?;

    let manifest_hash = sha256_hex(&manifest_bytes);
    let _persisted =
        persist_sync_success_best_effort(settings, manifest_hash, etag, persist_sync_success);
    Ok(serde_json::json!({ "status": "downloaded" }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &WebDavSyncSettings) -> Result<Option<Value>, AppError> {
    settings.validate()?;
    let auth = auth_for(settings);
    let manifest_url = remote_file_url(settings, REMOTE_MANIFEST)?;

    let Some((bytes, _)) = get_bytes(&manifest_url, &auth, MAX_MANIFEST_BYTES).await? else {
        return Ok(None);
    };

    let manifest: SyncManifest = serde_json::from_slice(&bytes).map_err(|e| AppError::Json {
        path: REMOTE_MANIFEST.to_string(),
        source: e,
    })?;

    let compatible = validate_manifest_compat(&manifest).is_ok();

    let payload = serde_json::json!({
        "deviceName": manifest.device_name,
        "createdAt": manifest.created_at,
        "snapshotId": manifest.snapshot_id,
        "version": manifest.version,
        "compatible": compatible,
        "artifacts": manifest.artifacts.keys().collect::<Vec<_>>(),
    });

    Ok(Some(payload))
}

// ─── Sync status persistence ─────────────────────────────────

fn persist_sync_success(
    settings: &mut WebDavSyncSettings,
    manifest_hash: String,
    etag: Option<String>,
) -> Result<(), AppError> {
    let status = WebDavSyncStatus {
        last_sync_at: Some(Utc::now().timestamp()),
        last_error: None,
        last_error_source: None,
        last_local_manifest_hash: Some(manifest_hash.clone()),
        last_remote_manifest_hash: Some(manifest_hash),
        last_remote_etag: etag,
    };
    settings.status = status.clone();
    update_webdav_sync_status(status)
}

// ─── Download & verify ───────────────────────────────────────

async fn download_and_verify(
    settings: &WebDavSyncSettings,
    auth: &WebDavAuth,
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

    let url = remote_file_url(settings, artifact_name)?;
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

fn remote_dir_segments(settings: &WebDavSyncSettings) -> Vec<String> {
    let mut segs = Vec::new();
    segs.extend(path_segments(&settings.remote_root).map(str::to_string));
    segs.push(format!("v{PROTOCOL_VERSION}"));
    segs.extend(path_segments(&settings.profile).map(str::to_string));
    segs
}

fn remote_file_url(settings: &WebDavSyncSettings, file_name: &str) -> Result<String, AppError> {
    let mut segs = remote_dir_segments(settings);
    segs.extend(path_segments(file_name).map(str::to_string));
    build_remote_url(&settings.base_url, &segs)
}

fn auth_for(settings: &WebDavSyncSettings) -> WebDavAuth {
    auth_from_credentials(&settings.username, &settings.password)
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_dir_segments_uses_v2() {
        let settings = WebDavSyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..WebDavSyncSettings::default()
        };
        let segs = remote_dir_segments(&settings);
        assert_eq!(segs, vec!["cc-switch-sync", "v2", "default"]);
    }
}
