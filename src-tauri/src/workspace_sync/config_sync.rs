//! Config-DB sync bundled into the unified workspace sync.
//!
//! cc-switch's own config (providers/settings/mcp/skills) is a **relational DB**,
//! not line-mergeable data, so it uses **whole-database "newer wins"** rather
//! than the per-file union in [`super::merge`]. It reuses the existing
//! [`crate::services::sync_protocol`] payload (`db.sql` + `skills.zip` +
//! `manifest.json`) but writes under the *unified* remote prefix
//! `{root}/v1/{profile}/config/` (same location as `workspace.zip`) via the
//! shared [`ObjectStorage`] transport, instead of the legacy
//! `{root}/v2/db-v6/{profile}/` object scheme.
//!
//! Conflict policy (per project decision): the side whose config snapshot has a
//! strictly newer `createdAt` wins the whole DB. We only import a remote
//! snapshot when it is newer than what we last synced *and* its
//! `dbCompatVersion` matches; otherwise we upload the local snapshot.

use bytes::Bytes;
use chrono::DateTime;

use crate::database::Database;
use crate::error::AppError;
use crate::services::sync_protocol::{
    self, build_local_snapshot, ArtifactMeta, SyncManifest, REMOTE_DB_SQL, REMOTE_MANIFEST,
    REMOTE_SKILLS_ZIP,
};
use crate::settings::WebDavSyncStatus;
use crate::workspace_sync::storage::ObjectStorage;

/// Outcome of the config phase, folded into the sync report.
#[derive(Debug, Clone, Default)]
pub struct ConfigSyncOutcome {
    /// "uploaded" | "downloaded" | "skipped" | "up-to-date"
    pub action: &'static str,
    /// The config snapshot's manifest `createdAt` after this phase (rfc3339).
    pub synced_at: Option<String>,
    /// The local snapshot manifest hash after this phase.
    pub local_hash: Option<String>,
}

fn config_prefix(remote_root: &str, profile: &str) -> String {
    format!("{}/v1/{}/config", remote_root.trim_end_matches('/'), profile)
}

/// A timestamp-independent identity of a config snapshot: hash of the two
/// artifact byte streams. (The manifest hash can't be used — it embeds
/// `createdAt`, so it changes on every build even for an identical DB.)
fn snapshot_content_hash(snap: &sync_protocol::LocalSnapshot) -> String {
    let mut buf = Vec::with_capacity(snap.db_sql.len() + snap.skills_zip.len());
    buf.extend_from_slice(&snap.db_sql);
    buf.extend_from_slice(&snap.skills_zip);
    sync_protocol::sha256_hex(&buf)
}

/// Cheap check for the scheduled-tick skip path: has the local config DB changed
/// since the last sync? Compares the freshly-built snapshot's content hash to
/// the stored `last_config_local_hash`. Returns `true` (unchanged) only on an
/// exact match; any error or missing marker returns `false` (i.e. sync).
pub fn local_config_unchanged(db: &Database, status: &WebDavSyncStatus) -> bool {
    let Some(prev) = status.last_config_local_hash.as_deref() else {
        return false;
    };
    match build_local_snapshot(db) {
        Ok(snap) => snapshot_content_hash(&snap) == prev,
        Err(_) => false,
    }
}

fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

/// Run the config-DB phase. `status` carries the last-synced markers so we can
/// decide upload vs download; it is updated in place on success.
///
/// Returns the outcome; the caller persists `status`.
pub async fn sync_config(
    storage: &dyn ObjectStorage,
    db: &Database,
    remote_root: &str,
    profile: &str,
    status: &mut WebDavSyncStatus,
) -> Result<ConfigSyncOutcome, AppError> {
    let prefix = config_prefix(remote_root, profile);
    storage.ensure_container(&prefix).await?;

    let manifest_key = format!("{prefix}/{REMOTE_MANIFEST}");

    // Build the local snapshot up front (we need its hash either way).
    let local = build_local_snapshot(db)?;
    let local_content_hash = snapshot_content_hash(&local);
    let local_changed = status
        .last_config_local_hash
        .as_deref()
        .map(|h| h != local_content_hash)
        .unwrap_or(true);

    // Inspect the remote manifest (if any).
    let remote_manifest: Option<SyncManifest> = match storage.get(&manifest_key).await? {
        Some(obj) => serde_json::from_slice(&obj.bytes).ok(),
        None => None,
    };

    // Decide: import remote if it is strictly newer than our last sync AND
    // compatible; else upload local (when it changed) or do nothing.
    if let Some(remote) = &remote_manifest {
        let remote_ms = parse_rfc3339_ms(&remote.created_at).unwrap_or(0);
        let last_ms = status
            .last_config_synced_at
            .as_deref()
            .and_then(parse_rfc3339_ms)
            .unwrap_or(0);

        let compatible = sync_protocol::validate_manifest_compat(
            remote,
            crate::services::sync_protocol::RemoteLayout::Current,
        )
        .is_ok();

        if remote_ms > last_ms && compatible && !local_changed {
            // Remote is newer and we have no un-synced local changes → import.
            return import_remote(storage, db, &prefix, remote, status).await;
        }

        if remote_ms > last_ms && compatible && local_changed {
            // Both sides changed: newer-wins by timestamp. Remote is newer →
            // import (local edits since last sync are overwritten by design).
            return import_remote(storage, db, &prefix, remote, status).await;
        }

        if !local_changed {
            // Nothing changed locally and remote isn't newer → up to date.
            return Ok(ConfigSyncOutcome {
                action: "up-to-date",
                synced_at: status.last_config_synced_at.clone(),
                local_hash: status.last_config_local_hash.clone(),
            });
        }
        // else: local changed and is newer-or-equal → fall through to upload.
    }

    // Upload the local snapshot (db.sql + skills.zip + manifest.json).
    upload_local(storage, &prefix, &local, status).await
}

async fn upload_local(
    storage: &dyn ObjectStorage,
    prefix: &str,
    local: &sync_protocol::LocalSnapshot,
    status: &mut WebDavSyncStatus,
) -> Result<ConfigSyncOutcome, AppError> {
    storage
        .put(
            &format!("{prefix}/{REMOTE_DB_SQL}"),
            Bytes::from(local.db_sql.clone()),
        )
        .await?;
    storage
        .put(
            &format!("{prefix}/{REMOTE_SKILLS_ZIP}"),
            Bytes::from(local.skills_zip.clone()),
        )
        .await?;
    // Manifest last (best-effort consistency marker).
    let put = storage
        .put(
            &format!("{prefix}/{REMOTE_MANIFEST}"),
            Bytes::from(local.manifest_bytes.clone()),
        )
        .await?;

    let created_at = serde_json::from_slice::<SyncManifest>(&local.manifest_bytes)
        .ok()
        .map(|m| m.created_at);
    let content_hash = snapshot_content_hash(local);
    status.last_config_synced_at = created_at.clone();
    status.last_config_local_hash = Some(content_hash.clone());
    if put.etag.is_some() {
        status.last_remote_etag = put.etag;
    }

    Ok(ConfigSyncOutcome {
        action: "uploaded",
        synced_at: created_at,
        local_hash: Some(content_hash),
    })
}

async fn import_remote(
    storage: &dyn ObjectStorage,
    db: &Database,
    prefix: &str,
    remote: &SyncManifest,
    status: &mut WebDavSyncStatus,
) -> Result<ConfigSyncOutcome, AppError> {
    let db_sql = fetch_verified(storage, prefix, REMOTE_DB_SQL, &remote.artifacts).await?;
    let skills_zip = fetch_verified(storage, prefix, REMOTE_SKILLS_ZIP, &remote.artifacts).await?;

    sync_protocol::apply_snapshot(db, &db_sql, &skills_zip)?;

    // After import, our local snapshot equals the remote; recompute its content
    // hash so a subsequent tick doesn't treat the imported state as a change.
    let local_after = build_local_snapshot(db).ok();
    status.last_config_synced_at = Some(remote.created_at.clone());
    status.last_config_local_hash = local_after.as_ref().map(snapshot_content_hash);

    Ok(ConfigSyncOutcome {
        action: "downloaded",
        synced_at: Some(remote.created_at.clone()),
        local_hash: status.last_config_local_hash.clone(),
    })
}

async fn fetch_verified(
    storage: &dyn ObjectStorage,
    prefix: &str,
    name: &str,
    artifacts: &std::collections::BTreeMap<String, ArtifactMeta>,
) -> Result<Vec<u8>, AppError> {
    let key = format!("{prefix}/{name}");
    let obj = storage.get(&key).await?.ok_or_else(|| {
        AppError::Message(format!("config sync: remote artifact {name} missing"))
    })?;
    let bytes = obj.bytes.to_vec();
    if let Some(meta) = artifacts.get(name) {
        sync_protocol::verify_artifact(&bytes, name, meta)?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_prefix_is_under_unified_v1_location() {
        assert_eq!(
            config_prefix("cc-switch-sync", "default"),
            "cc-switch-sync/v1/default/config"
        );
        // Trailing slash on root is trimmed.
        assert_eq!(
            config_prefix("root/", "prof"),
            "root/v1/prof/config"
        );
    }

    #[test]
    fn parse_rfc3339_orders_timestamps() {
        let older = parse_rfc3339_ms("2026-01-01T00:00:00Z").unwrap();
        let newer = parse_rfc3339_ms("2026-06-01T00:00:00+00:00").unwrap();
        assert!(newer > older);
        assert_eq!(parse_rfc3339_ms("not-a-date"), None);
    }
}

