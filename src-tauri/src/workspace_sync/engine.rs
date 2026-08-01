//! Workspace-sync orchestration: a single `sync()` that maintains the union of
//! every device's data on the cloud, transported as one archive file.
//!
//! Remote layout (under the transport's configured target):
//! ```text
//! <remote_root>/v1/<profile>/workspace.zip
//! ```
//! The archive holds `manifest.json` + `blobs/<sha256>` (see [`super::archive`]).
//!
//! One `sync()` does the whole cycle with ~2 network requests (1 GET + 1 PUT):
//! 1. download `workspace.zip` (if any) and unpack it in memory;
//! 2. for each selected provider, merge local scan × remote manifest
//!    (union + keep-both, see [`super::merge`]) and deploy the result to disk;
//! 3. repack the merged union and upload it, replacing the remote archive.
//!
//! This intentionally uses a single overwriting PUT rather than per-file
//! objects: it eliminates the request storm that trips cloud rate limits, and
//! since every sync pulls-then-merges first, the union converges across devices.

use std::future::Future;
use std::sync::OnceLock;

use bytes::Bytes;
use serde::Serialize;

use crate::error::AppError;
use crate::services::sync_protocol::detect_system_device_name;
use crate::settings::WorkspaceSyncSettings;
use crate::workspace_sync::adapters::{adapter_for, ProviderAdapter};
use crate::workspace_sync::archive::{self, LocalArchive};
use crate::workspace_sync::manifest::{SnapshotContent, SnapshotManifest};
use crate::workspace_sync::merge::{self, MaterializeAction};
use crate::workspace_sync::model::{DataItem, ProviderSnapshot, WorkspaceProviderId};
use crate::workspace_sync::storage::ObjectStorage;

const ARCHIVE_KEY: &str = "workspace.zip";

/// Serialize workspace-sync operations so runs never overlap.
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

/// Summary returned to the frontend after a sync.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SyncReport {
    pub snapshot_id: String,
    pub providers_scanned: usize,
    pub items_total: usize,
    /// Bytes of the uploaded archive (0 if nothing to upload).
    pub archive_bytes: usize,
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

fn archive_key(prefix: &str) -> String {
    format!("{prefix}/{ARCHIVE_KEY}")
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

// ─── Sync ────────────────────────────────────────────────────

/// Pull the remote union archive, merge with local, deploy locally, then push
/// the merged union back — replacing the remote archive.
pub async fn sync(
    storage: &dyn ObjectStorage,
    settings: &WorkspaceSyncSettings,
    now_ms: i64,
) -> Result<SyncReport, AppError> {
    let prefix = remote_prefix(settings);
    let device = device_name();
    let providers = selected_providers(settings);

    // MKCOL the directory chain so the GET below returns a clean 404 (not 409
    // on Jianguoyun) when the archive does not exist yet.
    storage.ensure_container(&prefix).await?;

    // 1) Download + unpack remote archive (if any).
    let remote_archive: Option<LocalArchive> =
        match storage.get(&archive_key(&prefix)).await? {
            Some(obj) => Some(archive::unpack_snapshot(&obj.bytes)?),
            None => None,
        };
    let remote_manifest = remote_archive.as_ref().map(|a| a.manifest().clone());
    let parents = remote_manifest
        .as_ref()
        .map(|m| vec![m.snapshot_id.clone()])
        .unwrap_or_default();

    let mut report = SyncReport::default();
    let mut snapshot_content = SnapshotContent {
        parents,
        providers: Default::default(),
        tombstones: Vec::new(),
    };

    // 2) Per-provider merge + local deploy.
    for provider in &providers {
        let adapter = adapter_for(*provider);
        report.providers_scanned += 1;

        let local_items = if adapter.is_installed() {
            adapter.scan()?
        } else {
            Vec::new()
        };
        let remote_items = remote_manifest
            .as_ref()
            .and_then(|m| m.content.providers.get(provider))
            .map(|s| s.items.clone())
            .unwrap_or_default();

        let remote_device = remote_manifest
            .as_ref()
            .map(|m| m.created_by.as_str())
            .unwrap_or("remote");

        let outcome =
            merge::merge_provider(&local_items, &remote_items, remote_device, &device);

        // Apply filesystem actions, reading remote blobs from the archive.
        apply_actions(
            &*adapter,
            remote_archive.as_ref(),
            &outcome.actions,
            &mut report,
        )?;

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

    // 3) Repack merged union and upload (one PUT), replacing the remote archive.
    let manifest = SnapshotManifest::new(snapshot_content, device, now_ms)?;

    // Blob bytes come from the merged local disk state (includes conflict
    // copies just written). Missing/unchanged remote-only blobs fall back to
    // the downloaded archive so they survive even if not present locally.
    let source = ArchiveBlobSource {
        manifest: &manifest,
        remote: remote_archive.as_ref(),
    };
    let hashes = collect_hashes(&manifest);
    let zip_bytes = archive::pack_snapshot(&manifest, &hashes, &source)?;
    report.archive_bytes = zip_bytes.len();

    storage
        .put(&archive_key(&prefix), Bytes::from(zip_bytes))
        .await?;

    report.snapshot_id = manifest.snapshot_id;
    Ok(report)
}

/// All distinct content hashes referenced by a manifest's items.
fn collect_hashes(manifest: &SnapshotManifest) -> Vec<String> {
    let mut hashes = Vec::new();
    for snapshot in manifest.content.providers.values() {
        for item in &snapshot.items {
            hashes.push(item.content_hash.clone());
        }
    }
    hashes.sort();
    hashes.dedup();
    hashes
}

/// Resolves blob bytes for packing: prefer live local files (by walking the
/// merged manifest's items via their adapters), else fall back to the remote
/// archive we just downloaded.
struct ArchiveBlobSource<'a> {
    manifest: &'a SnapshotManifest,
    remote: Option<&'a LocalArchive>,
}

impl archive::BlobSource for ArchiveBlobSource<'_> {
    fn read(&self, content_hash: &str) -> Option<Vec<u8>> {
        // Find an item with this hash and read it via its provider adapter.
        for snapshot in self.manifest.content.providers.values() {
            let adapter = adapter_for(snapshot.provider);
            for item in &snapshot.items {
                if item.content_hash == content_hash {
                    if let Ok(bytes) = adapter.read_blob(item) {
                        // Guard: only accept if it still matches the hash.
                        if crate::services::sync_protocol::sha256_hex(&bytes) == content_hash {
                            return Some(bytes);
                        }
                    }
                }
            }
        }
        // Fall back to the remote archive (e.g. other-device-only blobs whose
        // files are not present on this machine).
        self.remote
            .and_then(|a| a.read_blob(content_hash))
            .map(<[u8]>::to_vec)
    }
}

fn apply_actions(
    adapter: &dyn ProviderAdapter,
    remote: Option<&LocalArchive>,
    actions: &[MaterializeAction],
    report: &mut SyncReport,
) -> Result<(), AppError> {
    for action in actions {
        match action {
            MaterializeAction::FromRemoteBlob {
                content_hash,
                target_path,
            } => {
                let Some(bytes) = remote.and_then(|a| a.read_blob(content_hash)) else {
                    return Err(AppError::Message(format!(
                        "workspace sync: archive blob {content_hash} missing for {target_path}"
                    )));
                };
                let item = target_item(adapter.provider(), target_path, content_hash);
                adapter.materialize(&item, bytes)?;
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
) -> DataItem {
    use crate::workspace_sync::model::{DataKind, MergeCapability, Sensitivity};
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

fn source_item(provider: WorkspaceProviderId, native_path: &str) -> DataItem {
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
    async fn sync_roundtrip_pulls_remote_only_file_from_archive() {
        // Device A syncs a plan into the archive; device B (fresh) syncs it down.
        let home = TestHome::new();
        home.write("plans/roadmap.md", "phase 1");

        let storage = MemoryStorage::default();
        let s = settings();

        let first = sync(&storage, &s, 1000).await.expect("first sync");
        assert_eq!(first.providers_scanned, 1);
        assert!(first.items_total >= 1);
        assert!(first.archive_bytes > 0);

        // Simulate device B: remove local file, then sync should restore it
        // from the uploaded archive.
        fs::remove_file(home.claude_dir().join("plans/roadmap.md")).unwrap();
        let second = sync(&storage, &s, 2000).await.expect("second sync");
        assert_eq!(
            home.read("plans/roadmap.md").as_deref(),
            Some("phase 1"),
            "sync should restore the archived file to disk"
        );
        assert!(second.files_written >= 1);
    }

    #[tokio::test]
    #[serial]
    async fn sync_keeps_both_on_text_conflict() {
        let home = TestHome::new();

        let storage = MemoryStorage::default();
        let s = settings();

        // Seed remote with "remote version".
        home.write("plans/note.md", "remote version");
        sync(&storage, &s, 1000).await.expect("seed remote");

        // Local diverges.
        home.write("plans/note.md", "local edits again");
        let merged = sync(&storage, &s, 3000).await.expect("sync");

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
        let cpath = merged.conflicts[0]
            .conflict_path
            .clone()
            .expect("conflict path");
        assert!(cpath.contains(".conflict-"));
        assert_eq!(
            home.read(&cpath).as_deref(),
            Some("remote version"),
            "remote version should be saved to the conflict sibling"
        );
    }

    #[tokio::test]
    #[serial]
    async fn two_syncs_with_no_local_change_write_nothing_new() {
        let home = TestHome::new();
        home.write("plans/a.md", "content");
        let storage = MemoryStorage::default();
        let s = settings();

        let first = sync(&storage, &s, 1000).await.expect("first");
        assert!(first.files_written >= 0);
        // No local change → second sync deploys no files (local == remote union)
        // and the data union is unchanged (same item count). The snapshot_id
        // itself changes because the new snapshot parents the previous one.
        let second = sync(&storage, &s, 2000).await.expect("second");
        assert_eq!(
            second.files_written, 0,
            "no local change should write nothing back to disk"
        );
        assert_eq!(
            first.items_total, second.items_total,
            "the data union should be stable across idempotent syncs"
        );
    }
}
