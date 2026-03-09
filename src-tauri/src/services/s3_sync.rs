//! S3 v2 sync protocol layer.
//!
//! Implements manifest-based synchronization on top of the S3 transport
//! primitives in [`super::s3`]. Artifact set: `db.sql` + `skills.zip`.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::OnceLock;

use chrono::Utc;
use serde_json::Value;

use crate::error::AppError;
use crate::services::s3::{self, S3Credentials};
use crate::settings::{update_s3_sync_status, S3SyncSettings, WebDavSyncStatus};

use super::sync_protocol::{
    apply_snapshot, build_local_snapshot, localized, persist_sync_success_best_effort, sha256_hex,
    validate_artifact_size_limit, validate_manifest_compat, verify_artifact, ArtifactMeta,
    SyncManifest, MAX_MANIFEST_BYTES, MAX_SYNC_ARTIFACT_BYTES, PROTOCOL_VERSION, REMOTE_DB_SQL,
    REMOTE_MANIFEST, REMOTE_SKILLS_ZIP,
};

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

/// Check S3 connectivity by issuing a HEAD request against the bucket.
pub async fn check_connection(settings: &S3SyncSettings) -> Result<(), AppError> {
    settings.validate()?;
    let creds = creds_for(settings);
    s3::test_connection(&creds).await
}

/// Upload local snapshot (db + skills) to remote S3.
pub async fn upload(
    db: &crate::database::Database,
    settings: &mut S3SyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let creds = creds_for(settings);

    let snapshot = build_local_snapshot(db)?;

    // Upload order: artifacts first, manifest last (best-effort consistency)
    let db_key = s3_key(settings, REMOTE_DB_SQL);
    s3::put_object(&creds, &db_key, snapshot.db_sql, "application/sql").await?;

    let skills_key = s3_key(settings, REMOTE_SKILLS_ZIP);
    s3::put_object(&creds, &skills_key, snapshot.skills_zip, "application/zip").await?;

    let manifest_key = s3_key(settings, REMOTE_MANIFEST);
    s3::put_object(
        &creds,
        &manifest_key,
        snapshot.manifest_bytes,
        "application/json",
    )
    .await?;

    // Fetch etag (best-effort, don't fail the upload)
    let etag = match s3::head_object(&creds, &manifest_key).await {
        Ok(e) => e,
        Err(e) => {
            log::debug!("[S3] Failed to fetch ETag after upload: {e}");
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
    settings: &mut S3SyncSettings,
) -> Result<Value, AppError> {
    settings.validate()?;
    let creds = creds_for(settings);

    let manifest_key = s3_key(settings, REMOTE_MANIFEST);
    let (manifest_bytes, etag) = s3::get_object(&creds, &manifest_key, MAX_MANIFEST_BYTES)
        .await?
        .ok_or_else(|| {
            localized(
                "s3.sync.remote_empty",
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
    let db_sql = download_and_verify(settings, &creds, REMOTE_DB_SQL, &manifest.artifacts).await?;
    let skills_zip =
        download_and_verify(settings, &creds, REMOTE_SKILLS_ZIP, &manifest.artifacts).await?;

    // Apply snapshot
    apply_snapshot(db, &db_sql, &skills_zip)?;

    let manifest_hash = sha256_hex(&manifest_bytes);
    let _persisted =
        persist_sync_success_best_effort(settings, manifest_hash, etag, persist_sync_success);
    Ok(serde_json::json!({ "status": "downloaded" }))
}

/// Fetch remote manifest info without downloading artifacts.
pub async fn fetch_remote_info(settings: &S3SyncSettings) -> Result<Option<Value>, AppError> {
    settings.validate()?;
    let creds = creds_for(settings);
    let manifest_key = s3_key(settings, REMOTE_MANIFEST);

    let Some((bytes, _)) = s3::get_object(&creds, &manifest_key, MAX_MANIFEST_BYTES).await?
    else {
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
    settings: &mut S3SyncSettings,
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
    update_s3_sync_status(status)
}

// ─── Download & verify ───────────────────────────────────────

async fn download_and_verify(
    settings: &S3SyncSettings,
    creds: &S3Credentials,
    artifact_name: &str,
    artifacts: &BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let meta = artifacts.get(artifact_name).ok_or_else(|| {
        localized(
            "s3.sync.manifest_missing_artifact",
            format!("manifest 中缺少 artifact: {artifact_name}"),
            format!("Manifest missing artifact: {artifact_name}"),
        )
    })?;
    validate_artifact_size_limit(artifact_name, meta.size)?;

    let key = s3_key(settings, artifact_name);
    let (bytes, _) = s3::get_object(creds, &key, MAX_SYNC_ARTIFACT_BYTES as usize)
        .await?
        .ok_or_else(|| {
            localized(
                "s3.sync.remote_missing_artifact",
                format!("远端缺少 artifact 文件: {artifact_name}"),
                format!("Remote artifact file missing: {artifact_name}"),
            )
        })?;

    verify_artifact(&bytes, artifact_name, meta)?;
    Ok(bytes)
}

// ─── S3 key helpers ──────────────────────────────────────────

/// Build the S3 object key for a given artifact.
///
/// Format: `{remote_root}/v{PROTOCOL_VERSION}/{profile}/{artifact}`
/// Example: `cc-switch-sync/v2/default/manifest.json`
fn s3_key(settings: &S3SyncSettings, artifact: &str) -> String {
    format!(
        "{}/v{}/{}/{}",
        settings.remote_root, PROTOCOL_VERSION, settings.profile, artifact
    )
}

fn creds_for(settings: &S3SyncSettings) -> S3Credentials {
    S3Credentials {
        access_key_id: settings.access_key_id.clone(),
        secret_access_key: settings.secret_access_key.clone(),
        region: settings.region.clone(),
        bucket: settings.bucket.clone(),
        endpoint: settings.endpoint.clone(),
    }
}

// ─── Tests ───────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn test_settings() -> S3SyncSettings {
        S3SyncSettings {
            remote_root: "cc-switch-sync".to_string(),
            profile: "default".to_string(),
            ..S3SyncSettings::default()
        }
    }

    #[test]
    fn s3_key_uses_v2_and_correct_format() {
        let settings = test_settings();
        let key = s3_key(&settings, "manifest.json");
        assert_eq!(key, "cc-switch-sync/v2/default/manifest.json");
    }

    #[test]
    fn s3_key_with_custom_profile() {
        let settings = S3SyncSettings {
            remote_root: "my-root".to_string(),
            profile: "work".to_string(),
            ..S3SyncSettings::default()
        };
        assert_eq!(s3_key(&settings, "db.sql"), "my-root/v2/work/db.sql");
    }

    #[test]
    fn s3_key_matches_expected_pattern() {
        let settings = test_settings();
        let key = s3_key(&settings, "skills.zip");
        // Should follow {remote_root}/v{version}/{profile}/{artifact}
        let parts: Vec<&str> = key.splitn(4, '/').collect();
        assert_eq!(parts.len(), 4);
        assert_eq!(parts[0], "cc-switch-sync");
        assert_eq!(parts[1], "v2");
        assert_eq!(parts[2], "default");
        assert_eq!(parts[3], "skills.zip");
    }

    #[test]
    fn sync_mutex_is_singleton() {
        let m1 = sync_mutex();
        let m2 = sync_mutex();
        assert!(std::ptr::eq(m1, m2), "sync_mutex must return the same instance");
    }

    #[test]
    fn creds_for_maps_all_fields() {
        let settings = S3SyncSettings {
            access_key_id: "AKID".to_string(),
            secret_access_key: "SECRET".to_string(),
            region: "us-west-2".to_string(),
            bucket: "my-bucket".to_string(),
            endpoint: "minio.local:9000".to_string(),
            ..S3SyncSettings::default()
        };
        let creds = creds_for(&settings);
        assert_eq!(creds.access_key_id, "AKID");
        assert_eq!(creds.secret_access_key, "SECRET");
        assert_eq!(creds.region, "us-west-2");
        assert_eq!(creds.bucket, "my-bucket");
        assert_eq!(creds.endpoint, "minio.local:9000");
    }
}
