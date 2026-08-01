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

/// A cheap local fingerprint over the selected providers' files: a hash of
/// sorted `(native_path, content_hash)` pairs from a scan. Used to skip the
/// network round-trip on a scheduled tick when nothing changed locally.
///
/// This reuses the adapters' scan (which already hashes file contents), so it is
/// exact for local-change detection — not merely mtime-based.
pub fn compute_local_fingerprint(settings: &WorkspaceSyncSettings) -> String {
    use crate::services::sync_protocol::sha256_hex;
    let mut parts: Vec<String> = Vec::new();
    for provider in selected_providers(settings) {
        let adapter = adapter_for(provider);
        if !adapter.is_installed() {
            continue;
        }
        if let Ok(items) = adapter.scan() {
            for item in items {
                parts.push(format!(
                    "{}:{}:{}",
                    provider.as_str(),
                    item.native_path,
                    item.content_hash
                ));
            }
        }
    }
    parts.sort();
    sha256_hex(parts.join("\n").as_bytes())
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

        let outcome = merge::merge_provider(&local_items, &remote_items);

        // Apply filesystem actions, reading remote blobs from the archive.
        // Union merges return their new content hash so we can patch the
        // merged item (its hash isn't known until both blobs are combined).
        let mut merged_items = outcome.merged_items;
        let union_hashes = apply_actions(
            &*adapter,
            remote_archive.as_ref(),
            &outcome.actions,
            &mut report,
        )?;
        for item in merged_items.iter_mut() {
            if let Some(new_hash) = union_hashes.get(&item.native_path) {
                item.content_hash = new_hash.clone();
                item.object_ids = vec![new_hash.clone()];
            }
        }

        for c in outcome.conflicts {
            report.conflicts.push(ConflictReport {
                provider: provider.as_str().to_string(),
                logical_id: c.logical_id,
                resolution: c.resolution,
                conflict_path: None,
            });
        }

        report.items_total += merged_items.len();
        snapshot_content.providers.insert(
            *provider,
            ProviderSnapshot {
                provider: *provider,
                adapter_version: 1,
                native_version: None,
                schema_fingerprint: None,
                items: merged_items,
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

/// Apply filesystem actions. Returns a map of `native_path -> new content hash`
/// for line-union merges (whose hash isn't known until both blobs are combined),
/// so the caller can patch the corresponding merged items.
fn apply_actions(
    adapter: &dyn ProviderAdapter,
    remote: Option<&LocalArchive>,
    actions: &[MaterializeAction],
    report: &mut SyncReport,
) -> Result<std::collections::HashMap<String, String>, AppError> {
    let mut union_hashes = std::collections::HashMap::new();
    for action in actions {
        match action {
            MaterializeAction::FromRemoteBlob {
                content_hash,
                target_path,
                updated_at,
            } => {
                let Some(bytes) = remote.and_then(|a| a.read_blob(content_hash)) else {
                    return Err(AppError::Message(format!(
                        "workspace sync: archive blob {content_hash} missing for {target_path}"
                    )));
                };
                let mut item = target_item(adapter.provider(), target_path, content_hash);
                item.updated_at = *updated_at;
                adapter.materialize(&item, bytes)?;
                report.files_written += 1;
            }
            MaterializeAction::UnionMergeJsonl {
                native_path,
                remote_content_hash,
                remote_newer,
            } => {
                let local_item = source_item(adapter.provider(), native_path);
                let local_bytes = adapter.read_blob(&local_item).unwrap_or_default();
                let Some(remote_bytes) = remote.and_then(|a| a.read_blob(remote_content_hash))
                else {
                    return Err(AppError::Message(format!(
                        "workspace sync: archive blob {remote_content_hash} missing for {native_path}"
                    )));
                };
                let merged = merge::jsonl_union(&local_bytes, remote_bytes, *remote_newer);
                let new_hash = crate::services::sync_protocol::sha256_hex(&merged);
                let item = target_item(adapter.provider(), native_path, &new_hash);
                adapter.materialize(&item, &merged)?;
                report.files_written += 1;
                union_hashes.insert(native_path.clone(), new_hash);
            }
            MaterializeAction::MergeJsonUnion {
                native_path,
                remote_content_hash,
            } => {
                let local_item = source_item(adapter.provider(), native_path);
                let local_bytes = adapter.read_blob(&local_item).unwrap_or_default();
                let Some(remote_bytes) = remote.and_then(|a| a.read_blob(remote_content_hash))
                else {
                    return Err(AppError::Message(format!(
                        "workspace sync: archive blob {remote_content_hash} missing for {native_path}"
                    )));
                };
                let merged = merge::json_deep_union(&local_bytes, remote_bytes);
                let new_hash = crate::services::sync_protocol::sha256_hex(&merged);
                let item = target_item(adapter.provider(), native_path, &new_hash);
                adapter.materialize(&item, &merged)?;
                report.files_written += 1;
                union_hashes.insert(native_path.clone(), new_hash);
            }
        }
    }
    Ok(union_hashes)
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
            auto_sync: false,
            sync_interval_minutes: None,
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
    async fn sync_text_conflict_newer_wins_no_sidecar() {
        let home = TestHome::new();

        let storage = MemoryStorage::default();
        let s = settings();

        // Seed remote with "remote version".
        home.write("plans/note.md", "remote version");
        sync(&storage, &s, 1000).await.expect("seed remote");

        // Local diverges with a newer mtime (write happens after the seed sync).
        home.write("plans/note.md", "local edits again");
        let merged = sync(&storage, &s, 3000).await.expect("sync");

        // Newer local wins and is left in place.
        assert_eq!(
            home.read("plans/note.md").as_deref(),
            Some("local edits again")
        );
        // Divergence is reported as a conflict...
        assert!(
            !merged.conflicts.is_empty(),
            "text divergence should be reported"
        );
        // ...but NO .conflict sidecar file is produced (they re-propagate).
        let dir = std::fs::read_dir(home.claude_dir().join("plans")).unwrap();
        for entry in dir.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            assert!(
                !name.contains(".conflict-"),
                "no conflict sidecar expected, found {name}"
            );
        }
    }

    #[tokio::test]
    #[serial]
    async fn sync_history_jsonl_line_unions_both_devices() {
        let home = TestHome::new();
        let storage = MemoryStorage::default();
        let s = settings();

        // Device A seeds history with line "a".
        home.write("history.jsonl", "{\"e\":\"a\"}\n");
        sync(&storage, &s, 1000).await.expect("seed remote");

        // Device B (same home here) diverges: replace with a different line "b".
        home.write("history.jsonl", "{\"e\":\"b\"}\n");
        sync(&storage, &s, 3000).await.expect("union sync");

        // Merged file must contain BOTH devices' lines (line union, no clobber).
        let merged = home.read("history.jsonl").unwrap();
        assert!(merged.contains("{\"e\":\"a\"}"), "remote line kept: {merged}");
        assert!(merged.contains("{\"e\":\"b\"}"), "local line kept: {merged}");
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
