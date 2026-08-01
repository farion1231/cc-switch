//! Single-file workspace archive: pack a snapshot (manifest + content-addressed
//! blobs) into one `workspace.zip`, and unpack it back.
//!
//! This replaces the per-file remote object layout. Instead of hundreds of
//! HEAD/PUT/GET requests (which trips cloud rate limits like Jianguoyun), the
//! whole snapshot travels as a single object: one GET to download, one PUT to
//! upload. Mirrors the mature `services/webdav_sync/archive.rs` skills.zip
//! pattern, including its entry-count and total-size guards.
//!
//! Archive layout:
//! ```text
//! manifest.json          # serialized SnapshotManifest
//! blobs/<sha256>         # raw file contents, deduped by content hash
//! ```

use std::collections::HashMap;
use std::io::{Cursor, Read, Write};

use crate::error::AppError;
use crate::services::sync_protocol::{sha256_hex, MAX_SYNC_ARTIFACT_BYTES};
use crate::workspace_sync::manifest::SnapshotManifest;

use zip::write::SimpleFileOptions;
use zip::{DateTime, ZipArchive, ZipWriter};

const MANIFEST_ENTRY: &str = "manifest.json";
const BLOB_PREFIX: &str = "blobs/";
/// Maximum number of entries allowed in the archive (mirrors archive.rs).
const MAX_ENTRIES: usize = 200_000;

/// Provides blob bytes for a given content hash while packing.
pub trait BlobSource {
    /// Return the bytes for `content_hash`, or `None` if unavailable
    /// (unreadable/missing files are skipped rather than failing the pack).
    fn read(&self, content_hash: &str) -> Option<Vec<u8>>;
}

impl<F> BlobSource for F
where
    F: Fn(&str) -> Option<Vec<u8>>,
{
    fn read(&self, content_hash: &str) -> Option<Vec<u8>> {
        self(content_hash)
    }
}

/// Pack a manifest and the blobs it references into a deterministic zip.
///
/// `hashes` is the de-duplicated set of content hashes to include; bytes are
/// pulled from `source`. Hashes whose bytes are unavailable are skipped.
pub fn pack_snapshot(
    manifest: &SnapshotManifest,
    hashes: &[String],
    source: &dyn BlobSource,
) -> Result<Vec<u8>, AppError> {
    let mut buffer = Vec::new();
    {
        let mut writer = ZipWriter::new(Cursor::new(&mut buffer));
        let options = SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(DateTime::default());

        // manifest.json first.
        let manifest_bytes = serde_json::to_vec_pretty(manifest)
            .map_err(|source| AppError::JsonSerialize { source })?;
        writer
            .start_file(MANIFEST_ENTRY, options)
            .map_err(zip_err)?;
        writer.write_all(&manifest_bytes).map_err(|e| {
            AppError::Message(format!("failed to write manifest into archive: {e}"))
        })?;

        // Deterministic order: sorted, de-duplicated hashes.
        let mut sorted: Vec<&String> = hashes.iter().collect();
        sorted.sort();
        sorted.dedup();

        for hash in sorted {
            let Some(bytes) = source.read(hash) else {
                log::warn!("[workspace_sync] blob {hash} unavailable while packing; skipping");
                continue;
            };
            let entry = format!("{BLOB_PREFIX}{hash}");
            writer.start_file(entry, options).map_err(zip_err)?;
            writer.write_all(&bytes).map_err(|e| {
                AppError::Message(format!("failed to write blob {hash} into archive: {e}"))
            })?;
        }

        writer.finish().map_err(zip_err)?;
    }
    Ok(buffer)
}

/// An unpacked archive held in memory: the manifest plus a hash→bytes map.
#[derive(Debug)]
pub struct LocalArchive {
    manifest: SnapshotManifest,
    blobs: HashMap<String, Vec<u8>>,
}

impl LocalArchive {
    pub fn manifest(&self) -> &SnapshotManifest {
        &self.manifest
    }

    /// Bytes for a content hash, if present in the archive.
    pub fn read_blob(&self, content_hash: &str) -> Option<&[u8]> {
        self.blobs.get(content_hash).map(Vec::as_slice)
    }
}

/// Parse a `workspace.zip` into a [`LocalArchive`].
pub fn unpack_snapshot(zip_bytes: &[u8]) -> Result<LocalArchive, AppError> {
    let mut archive =
        ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| {
            AppError::Message(format!("failed to open workspace archive: {e}"))
        })?;

    if archive.len() > MAX_ENTRIES {
        return Err(AppError::Message(format!(
            "workspace archive has too many entries ({}), limit is {MAX_ENTRIES}",
            archive.len()
        )));
    }

    let mut manifest: Option<SnapshotManifest> = None;
    let mut blobs: HashMap<String, Vec<u8>> = HashMap::new();
    let mut total_bytes: u64 = 0;

    for idx in 0..archive.len() {
        let mut entry = archive.by_index(idx).map_err(|e| {
            AppError::Message(format!("failed to read archive entry: {e}"))
        })?;
        // enclosed_name guards against path traversal in entry names.
        let Some(name) = entry.enclosed_name() else {
            continue;
        };
        let name = name.to_string_lossy().replace('\\', "/");

        if entry.is_dir() {
            continue;
        }

        let mut bytes = Vec::new();
        read_with_total_limit(&mut entry, &mut bytes, &mut total_bytes)?;

        if name == MANIFEST_ENTRY {
            let parsed: SnapshotManifest =
                serde_json::from_slice(&bytes).map_err(|source| AppError::Json {
                    path: MANIFEST_ENTRY.to_string(),
                    source,
                })?;
            manifest = Some(parsed);
        } else if let Some(hash) = name.strip_prefix(BLOB_PREFIX) {
            if hash.is_empty() {
                continue;
            }
            // Integrity: the entry name must match its content hash.
            let actual = sha256_hex(&bytes);
            if actual != hash {
                return Err(AppError::Message(format!(
                    "workspace archive blob {hash} failed integrity check (got {actual})"
                )));
            }
            blobs.insert(hash.to_string(), bytes);
        }
        // Unknown entries are ignored for forward-compat.
    }

    let manifest = manifest.ok_or_else(|| {
        AppError::Message("workspace archive missing manifest.json".to_string())
    })?;

    Ok(LocalArchive { manifest, blobs })
}

fn read_with_total_limit<R: Read>(
    reader: &mut R,
    out: &mut Vec<u8>,
    total_bytes: &mut u64,
) -> Result<(), AppError> {
    let mut buf = [0u8; 16 * 1024];
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| AppError::Message(format!("failed to read archive entry: {e}")))?;
        if n == 0 {
            break;
        }
        *total_bytes = total_bytes.saturating_add(n as u64);
        if *total_bytes > MAX_SYNC_ARTIFACT_BYTES {
            let max_mb = MAX_SYNC_ARTIFACT_BYTES / 1024 / 1024;
            return Err(AppError::Message(format!(
                "workspace archive exceeds size limit ({max_mb} MB)"
            )));
        }
        out.extend_from_slice(&buf[..n]);
    }
    Ok(())
}

fn zip_err(e: zip::result::ZipError) -> AppError {
    AppError::Message(format!("workspace archive zip error: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace_sync::manifest::SnapshotContent;
    use std::collections::BTreeMap;

    fn empty_manifest() -> SnapshotManifest {
        SnapshotManifest::new(
            SnapshotContent {
                parents: Vec::new(),
                providers: BTreeMap::new(),
                tombstones: Vec::new(),
            },
            "test-device",
            1000,
        )
        .expect("manifest")
    }

    #[test]
    fn pack_then_unpack_roundtrips_manifest_and_blobs() {
        let manifest = empty_manifest();
        let hello_hash = sha256_hex(b"hello");
        let world_hash = sha256_hex(b"world");
        let store: HashMap<String, Vec<u8>> = [
            (hello_hash.clone(), b"hello".to_vec()),
            (world_hash.clone(), b"world".to_vec()),
        ]
        .into_iter()
        .collect();

        let source = move |h: &str| store.get(h).cloned();
        let zip = pack_snapshot(
            &manifest,
            &[hello_hash.clone(), world_hash.clone()],
            &source,
        )
        .expect("pack");

        let archive = unpack_snapshot(&zip).expect("unpack");
        assert_eq!(archive.manifest().snapshot_id, manifest.snapshot_id);
        assert_eq!(archive.read_blob(&hello_hash), Some(&b"hello"[..]));
        assert_eq!(archive.read_blob(&world_hash), Some(&b"world"[..]));
        assert_eq!(archive.read_blob("missing"), None);
    }

    #[test]
    fn unpack_rejects_blob_with_wrong_hash() {
        // Hand-build an archive whose blob name mismatches its content.
        let manifest = empty_manifest();
        let mut buffer = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default();
            let mbytes = serde_json::to_vec(&manifest).unwrap();
            writer.start_file(MANIFEST_ENTRY, options).unwrap();
            writer.write_all(&mbytes).unwrap();
            writer.start_file("blobs/deadbeef", options).unwrap();
            writer.write_all(b"not-deadbeef").unwrap();
            writer.finish().unwrap();
        }
        let err = unpack_snapshot(&buffer).expect_err("should reject tampered blob");
        assert!(err.to_string().contains("integrity"));
    }

    #[test]
    fn unpack_requires_manifest() {
        let mut buffer = Vec::new();
        {
            let mut writer = ZipWriter::new(Cursor::new(&mut buffer));
            let options = SimpleFileOptions::default();
            let h = sha256_hex(b"x");
            writer.start_file(format!("blobs/{h}"), options).unwrap();
            writer.write_all(b"x").unwrap();
            writer.finish().unwrap();
        }
        let err = unpack_snapshot(&buffer).expect_err("missing manifest");
        assert!(err.to_string().contains("missing manifest"));
    }

    #[test]
    fn pack_skips_unavailable_blobs() {
        let manifest = empty_manifest();
        let source = |_h: &str| None::<Vec<u8>>;
        let zip = pack_snapshot(&manifest, &["abc".to_string()], &source).expect("pack");
        let archive = unpack_snapshot(&zip).expect("unpack");
        assert_eq!(archive.read_blob("abc"), None);
    }
}
