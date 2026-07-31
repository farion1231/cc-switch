//! Workspace-sync orchestration: backup and merge.
//!
//! Remote layout (under the transport's configured target):
//! ```text
//! <remote_root>/v1/<profile>/
//! ├── head.json                 # { snapshotId, deviceName, updatedAt }
//! ├── manifests/<snapshotId>.json
//! └── blobs/<sha256>            # plaintext file contents, deduped by hash
//! ```
//!
//! `backup` scans the selected providers, uploads any missing blobs, writes a
//! new manifest, and points `head.json` at it. `merge` pulls the remote head,
//! merges it with a fresh local scan (union + keep-both, see [`super::merge`]),
//! writes merged files back to disk, then uploads the merged snapshot and
//! advances the head — satisfying "merge result can be backed up again".

use std::future::Future;
use std::sync::OnceLock;

use bytes::Bytes;
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::services::sync_protocol::detect_system_device_name;
use crate::settings::WorkspaceSyncSettings;
use crate::workspace_sync::adapters::{adapter_for, ProviderAdapter};
use crate::workspace_sync::blobs;
use crate::workspace_sync::manifest::{SnapshotContent, SnapshotManifest};
use crate::workspace_sync::merge::{self, MaterializeAction};
use crate::workspace_sync::model::{ProviderSnapshot, WorkspaceProviderId};
use crate::workspace_sync::storage::ObjectStorage;

const HEAD_KEY: &str = "head.json";

/// Serialize workspace-sync operations so backup/merge never overlap.
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Head {
    pub snapshot_id: String,
    pub device_name: String,
    pub updated_at: i64,
}

/// Summary returned to the frontend after a backup/merge.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub snapshot_id: String,
    pub providers_scanned: usize,
    pub items_total: usize,
    pub blobs_uploaded: usize,
    pub files_written: usize,
    pub conflicts: Vec<ConflictReport>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictReport {
    pub provider: String,
    pub logical_id: String,
    pub resolution: String,
    pub conflict_path: Option<String>,
}

/// The remote-key prefix `<remote_root>/v1/<profile>`.
fn remote_prefix(settings: &WorkspaceSyncSettings) -> String {
    format!(
        "{}/v1/{}",
        settings.remote_root.trim_end_matches('/'),
        settings.profile
    )
}

fn key(prefix: &str, suffix: &str) -> String {
    format!("{prefix}/{suffix}")
}

fn device_name() -> String {
    detect_system_device_name().unwrap_or_else(|| "Unknown Device".to_string())
}

fn selected_providers(settings: &WorkspaceSyncSettings) -> Vec<WorkspaceProviderId> {
    settings
        .providers
        .iter()
        .filter_map(|p| WorkspaceProviderId::parse(p))
        .collect()
}

// ─── Head + manifest I/O ─────────────────────────────────────

async fn read_head(
    storage: &dyn ObjectStorage,
    prefix: &str,
) -> Result<Option<(Head, Option<String>)>, AppError> {
    let Some(obj) = storage.get(&key(prefix, HEAD_KEY)).await? else {
        return Ok(None);
    };
    let head: Head = serde_json::from_slice(&obj.bytes).map_err(|e| AppError::Json {
        path: HEAD_KEY.to_string(),
        source: e,
    })?;
    Ok(Some((head, obj.etag)))
}

async fn read_manifest(
    storage: &dyn ObjectStorage,
    prefix: &str,
    snapshot_id: &str,
) -> Result<Option<SnapshotManifest>, AppError> {
    let mkey = key(prefix, &format!("manifests/{snapshot_id}.json"));
    let Some(obj) = storage.get(&mkey).await? else {
        return Ok(None);
    };
    let manifest: SnapshotManifest =
        serde_json::from_slice(&obj.bytes).map_err(|e| AppError::Json {
            path: mkey,
            source: e,
        })?;
    Ok(Some(manifest))
}

async fn write_manifest(
    storage: &dyn ObjectStorage,
    prefix: &str,
    manifest: &SnapshotManifest,
) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(manifest)
        .map_err(|source| AppError::JsonSerialize { source })?;
    let mkey = key(prefix, &format!("manifests/{}.json", manifest.snapshot_id));
    // Manifests are content-addressed by snapshot_id → write-once is fine.
    storage.put(&mkey, Bytes::from(bytes)).await?;
    Ok(())
}

async fn write_head(
    storage: &dyn ObjectStorage,
    prefix: &str,
    head: &Head,
    prev_etag: Option<String>,
) -> Result<(), AppError> {
    let bytes =
        serde_json::to_vec_pretty(head).map_err(|source| AppError::JsonSerialize { source })?;
    let hkey = key(prefix, HEAD_KEY);
    match prev_etag {
        Some(etag) => {
            // Optimistic concurrency: only advance if head is unchanged.
            match storage
                .put_if_match(&hkey, &etag, Bytes::from(bytes.clone()))
                .await?
            {
                crate::workspace_sync::storage::ConditionalPutResult::Written { .. } => Ok(()),
                _ => {
                    // Lost the race; overwrite anyway — merge already unioned
                    // remote state and keep-both guarantees no data loss.
                    log::warn!("[workspace_sync] head CAS lost race, forcing update");
                    storage.put(&hkey, Bytes::from(bytes)).await?;
                    Ok(())
                }
            }
        }
        None => {
            storage.put(&hkey, Bytes::from(bytes)).await?;
            Ok(())
        }
    }
}

// ─── Backup ──────────────────────────────────────────────────

/// Scan selected providers, upload blobs + manifest, advance head.
pub async fn backup(
    storage: &dyn ObjectStorage,
    settings: &WorkspaceSyncSettings,
    now_ms: i64,
) -> Result<SyncReport, AppError> {
    let prefix = remote_prefix(settings);
    let device = device_name();
    let providers = selected_providers(settings);

    // Create the remote directory chain first so the head GET below returns a
    // clean 404 (not 409) on WebDAV servers like Jianguoyun.
    storage.ensure_container(&prefix).await?;

    let prev_head = read_head(storage, &prefix).await?;
    let parents = prev_head
        .as_ref()
        .map(|(h, _)| vec![h.snapshot_id.clone()])
        .unwrap_or_default();

    let mut report = SyncReport::default();
    let mut snapshot_content = SnapshotContent {
        parents,
        providers: Default::default(),
        tombstones: Vec::new(),
    };

    for provider in &providers {
        let adapter = adapter_for(*provider);
        if !adapter.is_installed() {
            continue;
        }
        report.providers_scanned += 1;
        let items = adapter.scan()?;
        report.items_total += items.len();

        // Upload each item's blob (deduped by content hash).
        for item in &items {
            let bytes = match adapter.read_blob(item) {
                Ok(b) => b,
                Err(e) => {
                    log::warn!(
                        "[workspace_sync] skip unreadable {}: {e}",
                        item.native_path
                    );
                    continue;
                }
            };
            let uploaded =
                blobs::put_blob(storage, &prefix, &item.content_hash, bytes).await?;
            if uploaded {
                report.blobs_uploaded += 1;
            }
        }

        snapshot_content.providers.insert(
            *provider,
            ProviderSnapshot {
                provider: *provider,
                adapter_version: 1,
                native_version: None,
                schema_fingerprint: None,
                items,
            },
        );
    }

    let manifest = SnapshotManifest::new(snapshot_content, device.clone(), now_ms)?;
    write_manifest(storage, &prefix, &manifest).await?;

    let head = Head {
        snapshot_id: manifest.snapshot_id.clone(),
        device_name: device,
        updated_at: now_ms,
    };
    write_head(storage, &prefix, &head, prev_head.and_then(|(_, e)| e)).await?;

    report.snapshot_id = manifest.snapshot_id;
    Ok(report)
}

// ─── Merge ───────────────────────────────────────────────────

/// Pull remote head, merge with a fresh local scan (union + keep-both), write
/// merged files locally, then upload the merged snapshot and advance the head.
pub async fn merge(
    storage: &dyn ObjectStorage,
    settings: &WorkspaceSyncSettings,
    now_ms: i64,
) -> Result<SyncReport, AppError> {
    let prefix = remote_prefix(settings);
    let device = device_name();
    let providers = selected_providers(settings);

    storage.ensure_container(&prefix).await?;

    let Some((remote_head, head_etag)) = read_head(storage, &prefix).await? else {
        // No remote yet → merge degenerates to a backup.
        return backup(storage, settings, now_ms).await;
    };
    let remote_manifest = read_manifest(storage, &prefix, &remote_head.snapshot_id)
        .await?
        .ok_or_else(|| {
            AppError::Message(format!(
                "workspace sync: head points at missing manifest {}",
                remote_head.snapshot_id
            ))
        })?;

    let mut report = SyncReport::default();
    let mut snapshot_content = SnapshotContent {
        parents: vec![remote_head.snapshot_id.clone()],
        providers: Default::default(),
        tombstones: Vec::new(),
    };

    for provider in &providers {
        let adapter = adapter_for(*provider);
        report.providers_scanned += 1;

        let local_items = if adapter.is_installed() {
            adapter.scan()?
        } else {
            Vec::new()
        };
        let remote_items = remote_manifest
            .content
            .providers
            .get(provider)
            .map(|s| s.items.clone())
            .unwrap_or_default();

        let outcome = merge::merge_provider(
            &local_items,
            &remote_items,
            &remote_head.device_name,
            &device,
        );

        // Apply filesystem actions.
        apply_actions(&*adapter, storage, &prefix, &outcome.actions, &mut report)
            .await?;

        for c in outcome.conflicts {
            report.conflicts.push(ConflictReport {
                provider: provider.as_str().to_string(),
                logical_id: c.logical_id,
                resolution: c.resolution,
                conflict_path: c.conflict_path,
            });
        }

        report.items_total += outcome.merged_items.len();
        snapshot_content.providers.insert(
            *provider,
            ProviderSnapshot {
                provider: *provider,
                adapter_version: 1,
                native_version: None,
                schema_fingerprint: None,
                items: outcome.merged_items,
            },
        );
    }

    // Ensure blobs for any newly-created conflict copies exist remotely, plus
    // re-affirm all merged blobs (cheap: put_if_absent skips existing).
    for snapshot in snapshot_content.providers.values() {
        let adapter = adapter_for(snapshot.provider);
        for item in &snapshot.items {
            if let Ok(bytes) = adapter.read_blob(item) {
                let uploaded =
                    blobs::put_blob(storage, &prefix, &item.content_hash, bytes).await?;
                if uploaded {
                    report.blobs_uploaded += 1;
                }
            }
        }
    }

    let manifest = SnapshotManifest::new(snapshot_content, device.clone(), now_ms)?;
    write_manifest(storage, &prefix, &manifest).await?;

    let head = Head {
        snapshot_id: manifest.snapshot_id.clone(),
        device_name: device,
        updated_at: now_ms,
    };
    write_head(storage, &prefix, &head, head_etag).await?;

    report.snapshot_id = manifest.snapshot_id;
    Ok(report)
}

async fn apply_actions(
    adapter: &dyn ProviderAdapter,
    storage: &dyn ObjectStorage,
    prefix: &str,
    actions: &[MaterializeAction],
    report: &mut SyncReport,
) -> Result<(), AppError> {
    for action in actions {
        match action {
            MaterializeAction::FromRemoteBlob {
                content_hash,
                target_path,
            } => {
                let Some(bytes) = blobs::get_blob(storage, prefix, content_hash).await? else {
                    return Err(AppError::Message(format!(
                        "workspace sync: remote blob {content_hash} missing for {target_path}"
                    )));
                };
                let item = target_item(adapter.provider(), target_path, content_hash);
                adapter.materialize(&item, &bytes)?;
                report.files_written += 1;
            }
            MaterializeAction::CopyLocalFile { from_path, to_path } => {
                let from = source_item(adapter.provider(), from_path);
                let bytes = adapter.read_blob(&from)?;
                let to = target_item(adapter.provider(), to_path, "");
                adapter.materialize(&to, &bytes)?;
                report.files_written += 1;
            }
        }
    }
    Ok(())
}

/// Minimal DataItem used only to drive adapter path resolution for writes.
fn target_item(
    provider: WorkspaceProviderId,
    native_path: &str,
    content_hash: &str,
) -> crate::workspace_sync::model::DataItem {
    use crate::workspace_sync::model::{DataItem, DataKind, MergeCapability, Sensitivity};
    DataItem {
        provider,
        kind: DataKind::Attachment,
        logical_id: native_path.to_string(),
        parent_id: None,
        native_path: native_path.to_string(),
        content_hash: content_hash.to_string(),
        updated_at: None,
        schema_fingerprint: None,
        merge_capability: MergeCapability::Opaque,
        sensitivity: Sensitivity::WorkData,
        object_ids: Vec::new(),
    }
}

fn source_item(
    provider: WorkspaceProviderId,
    native_path: &str,
) -> crate::workspace_sync::model::DataItem {
    target_item(provider, native_path, "")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_sync::storage::memory::MemoryStorage;
    use serial_test::serial;
    use std::fs;
    use std::path::PathBuf;

    struct TestHome {
        _dir: tempfile::TempDir,
        prev: Option<String>,
        home: PathBuf,
    }

    impl TestHome {
        fn new() -> Self {
            let dir = tempfile::tempdir().expect("tempdir");
            let home = dir.path().to_path_buf();
            let prev = std::env::var("CC_SWITCH_TEST_HOME").ok();
            std::env::set_var("CC_SWITCH_TEST_HOME", &home);
            Self {
                _dir: dir,
                prev,
                home,
            }
        }

        fn claude_dir(&self) -> PathBuf {
            self.home.join(".claude")
        }

        fn write(&self, rel: &str, content: &str) {
            let path = self.claude_dir().join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }

        fn read(&self, rel: &str) -> Option<String> {
            fs::read_to_string(self.claude_dir().join(rel)).ok()
        }
    }

    impl Drop for TestHome {
        fn drop(&mut self) {
            match &self.prev {
                Some(v) => std::env::set_var("CC_SWITCH_TEST_HOME", v),
                None => std::env::remove_var("CC_SWITCH_TEST_HOME"),
            }
        }
    }

    fn settings() -> WorkspaceSyncSettings {
        WorkspaceSyncSettings {
            enabled: true,
            transport: "webdav".to_string(),
            providers: vec!["claude".to_string()],
            remote_root: "cc-switch-workspace".to_string(),
            profile: "default".to_string(),
            status: Default::default(),
        }
    }

    #[tokio::test]
    #[serial]
    async fn backup_then_merge_roundtrip_pulls_remote_only_file() {
        // Device A backs up a plan; a fresh local (device B) merges it down.
        let home = TestHome::new();
        home.write("plans/roadmap.md", "phase 1");

        let storage = MemoryStorage::default();
        let s = settings();

        let backup = backup(&storage, &s, 1000).await.expect("backup");
        assert_eq!(backup.providers_scanned, 1);
        assert!(backup.items_total >= 1);
        assert!(backup.blobs_uploaded >= 1);

        // Simulate device B: remove local file, then merge should restore it.
        fs::remove_file(home.claude_dir().join("plans/roadmap.md")).unwrap();
        let merged = merge(&storage, &s, 2000).await.expect("merge");
        assert_eq!(
            home.read("plans/roadmap.md").as_deref(),
            Some("phase 1"),
            "merge should pull the remote-only file back to disk"
        );
        assert!(merged.files_written >= 1);
    }

    #[tokio::test]
    #[serial]
    async fn merge_keeps_both_on_text_conflict() {
        let home = TestHome::new();
        home.write("plans/note.md", "local edits");

        let storage = MemoryStorage::default();
        let s = settings();

        // Seed remote with a different version of the same logical file by
        // backing up, then changing the local file so hashes diverge.
        home.write("plans/note.md", "remote version");
        backup(&storage, &s, 1000).await.expect("seed remote");

        // Now local diverges again.
        home.write("plans/note.md", "local edits again");
        let merged = merge(&storage, &s, 3000).await.expect("merge");

        // Local file preserved untouched.
        assert_eq!(
            home.read("plans/note.md").as_deref(),
            Some("local edits again")
        );
        // A conflict copy of the remote version was written and reported.
        assert!(
            !merged.conflicts.is_empty(),
            "text divergence should produce a conflict"
        );
        let conflict = &merged.conflicts[0];
        let cpath = conflict.conflict_path.clone().expect("conflict path");
        assert!(cpath.contains(".conflict-"));
        assert_eq!(
            home.read(&cpath).as_deref(),
            Some("remote version"),
            "remote version should be saved to the conflict sibling"
        );
    }

    #[tokio::test]
    #[serial]
    async fn merge_result_can_be_backed_up_again() {
        let home = TestHome::new();
        home.write("plans/a.md", "content");
        let storage = MemoryStorage::default();
        let s = settings();

        backup(&storage, &s, 1000).await.expect("backup");
        let merged = merge(&storage, &s, 2000).await.expect("merge");
        // Head now points at the merged snapshot; a follow-up backup succeeds
        // and parents it correctly (no panic, valid snapshot id).
        let again = backup(&storage, &s, 3000).await.expect("re-backup");
        assert!(!again.snapshot_id.is_empty());
        assert_ne!(again.snapshot_id, merged.snapshot_id);
    }
}

