//! Provider adapters: read/write each tool's on-disk workspace data.
//!
//! Every adapter maps a provider's config directory to a flat list of
//! [`DataItem`]s (one per session/plan/memory/… file) and can materialize items
//! back to disk. The heavy lifting is generic: [`FsAdapter`] walks a declarative
//! set of [`Mapping`]s (a subdir or a specific file → [`DataKind`] +
//! [`MergeCapability`]), so per-provider modules only declare their layout.
//!
//! Directory resolution reuses the existing `get_*_config_dir()` helpers so
//! per-app overrides (`CC_SWITCH_TEST_HOME`, settings overrides) apply here too.

pub mod claude;
pub mod codex;
pub mod cursor;
pub mod grokbuild;
pub mod opencode;

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::error::AppError;
use crate::services::sync_protocol::{sha256_hex, MAX_SYNC_ARTIFACT_BYTES};
use crate::workspace_sync::model::{
    DataItem, DataKind, MergeCapability, Sensitivity, WorkspaceProviderId,
};

/// Per-file size ceiling; oversized files are skipped with a warning rather than
/// aborting the whole scan.
const MAX_ITEM_BYTES: u64 = MAX_SYNC_ARTIFACT_BYTES;
/// Safety cap on files scanned per provider, mirroring `archive.rs`.
const MAX_ITEMS_PER_PROVIDER: usize = 50_000;

/// Reads and writes one provider's workspace data.
pub trait ProviderAdapter: Send + Sync {
    fn provider(&self) -> WorkspaceProviderId;

    /// Whether the provider's root directory exists on this machine.
    fn is_installed(&self) -> bool;

    /// Scan local data into [`DataItem`]s. `object_ids` holds the content hash
    /// (blob key); the blob store uploads the bytes separately.
    fn scan(&self) -> Result<Vec<DataItem>, AppError>;

    /// Write `bytes` back to the item's native path (creating parent dirs).
    fn materialize(&self, item: &DataItem, bytes: &[u8]) -> Result<(), AppError>;

    /// Read the current on-disk bytes for an item (used to upload blobs).
    fn read_blob(&self, item: &DataItem) -> Result<Vec<u8>, AppError>;
}

/// A single "source" within a provider root.
#[derive(Debug, Clone, Copy)]
pub struct Mapping {
    /// Relative path from the provider root. May be a directory (walked
    /// recursively) or a single file.
    pub rel: &'static str,
    pub kind: DataKind,
    pub capability: MergeCapability,
    /// If non-empty, only files with these extensions (no dot) are included.
    pub extensions: &'static [&'static str],
}

impl Mapping {
    pub const fn dir(rel: &'static str, kind: DataKind, capability: MergeCapability) -> Self {
        Self {
            rel,
            kind,
            capability,
            extensions: &[],
        }
    }

    pub const fn dir_ext(
        rel: &'static str,
        kind: DataKind,
        capability: MergeCapability,
        extensions: &'static [&'static str],
    ) -> Self {
        Self {
            rel,
            kind,
            capability,
            extensions,
        }
    }

    pub const fn file(rel: &'static str, kind: DataKind, capability: MergeCapability) -> Self {
        Self {
            rel,
            kind,
            capability,
            extensions: &[],
        }
    }
}

/// Generic filesystem-backed adapter driven by declarative [`Mapping`]s.
pub struct FsAdapter {
    provider: WorkspaceProviderId,
    root: PathBuf,
    mappings: &'static [Mapping],
    adapter_version: u32,
}

impl FsAdapter {
    pub fn new(
        provider: WorkspaceProviderId,
        root: PathBuf,
        mappings: &'static [Mapping],
    ) -> Self {
        Self {
            provider,
            root,
            mappings,
            adapter_version: 1,
        }
    }

    /// Resolve an item's native path and reject anything escaping the root.
    fn resolve_within_root(&self, native_path: &str) -> Result<PathBuf, AppError> {
        // native_path is provider-root-relative and must stay inside the root.
        let rel = Path::new(native_path);
        for comp in rel.components() {
            match comp {
                Component::Normal(_) | Component::CurDir => {}
                _ => {
                    return Err(AppError::Message(format!(
                        "workspace sync: illegal native path escaping root: {native_path}"
                    )))
                }
            }
        }
        Ok(self.root.join(rel))
    }
}

impl ProviderAdapter for FsAdapter {
    fn provider(&self) -> WorkspaceProviderId {
        self.provider
    }

    fn is_installed(&self) -> bool {
        self.root.is_dir()
    }

    fn scan(&self) -> Result<Vec<DataItem>, AppError> {
        let mut items = Vec::new();
        if !self.root.is_dir() {
            return Ok(items);
        }
        let canonical_root = fs::canonicalize(&self.root).unwrap_or_else(|_| self.root.clone());

        for mapping in self.mappings {
            let target = self.root.join(mapping.rel);
            if target.is_file() {
                if let Some(item) =
                    build_item(self.provider, &canonical_root, &target, mapping)?
                {
                    items.push(item);
                }
            } else if target.is_dir() {
                walk_dir(
                    self.provider,
                    &canonical_root,
                    &target,
                    mapping,
                    &mut items,
                    &mut HashSet::new(),
                )?;
            }
            if items.len() > MAX_ITEMS_PER_PROVIDER {
                log::warn!(
                    "[workspace_sync] {:?} exceeded {MAX_ITEMS_PER_PROVIDER} items; truncating scan",
                    self.provider
                );
                items.truncate(MAX_ITEMS_PER_PROVIDER);
                break;
            }
        }
        Ok(items)
    }

    fn materialize(&self, item: &DataItem, bytes: &[u8]) -> Result<(), AppError> {
        let path = self.resolve_within_root(&item.native_path)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }
        fs::write(&path, bytes).map_err(|e| AppError::io(&path, e))?;
        Ok(())
    }

    fn read_blob(&self, item: &DataItem) -> Result<Vec<u8>, AppError> {
        let path = self.resolve_within_root(&item.native_path)?;
        fs::read(&path).map_err(|e| AppError::io(&path, e))
    }
}

fn walk_dir(
    provider: WorkspaceProviderId,
    canonical_root: &Path,
    dir: &Path,
    mapping: &Mapping,
    items: &mut Vec<DataItem>,
    visited: &mut HashSet<PathBuf>,
) -> Result<(), AppError> {
    // Guard against symlink cycles, mirroring archive.rs.
    let canonical_dir = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canonical_dir.clone()) {
        return Ok(());
    }

    let mut entries: Vec<_> = fs::read_dir(dir)
        .map_err(|e| AppError::io(dir, e))?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(|e| e.file_name());

    for entry in entries {
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') {
            continue;
        }

        // Reject symlinks pointing outside the provider root.
        let real = match fs::canonicalize(&path) {
            Ok(p) if p.starts_with(canonical_root) => p,
            Ok(_) => {
                log::warn!(
                    "[workspace_sync] skipping symlink outside root: {}",
                    path.display()
                );
                continue;
            }
            Err(_) => path.clone(),
        };

        if real.is_dir() {
            walk_dir(provider, canonical_root, &real, mapping, items, visited)?;
        } else if let Some(item) = build_item(provider, canonical_root, &path, mapping)? {
            items.push(item);
        }
    }
    Ok(())
}

fn build_item(
    provider: WorkspaceProviderId,
    canonical_root: &Path,
    file: &Path,
    mapping: &Mapping,
) -> Result<Option<DataItem>, AppError> {
    // Extension filter.
    if !mapping.extensions.is_empty() {
        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        match ext {
            Some(e) if mapping.extensions.contains(&e.as_str()) => {}
            _ => return Ok(None),
        }
    }

    let meta = fs::metadata(file).map_err(|e| AppError::io(file, e))?;
    if meta.len() > MAX_ITEM_BYTES {
        log::warn!(
            "[workspace_sync] skipping oversized file ({} bytes): {}",
            meta.len(),
            file.display()
        );
        return Ok(None);
    }

    let bytes = fs::read(file).map_err(|e| AppError::io(file, e))?;
    let content_hash = sha256_hex(&bytes);

    // native_path / logical_id are provider-root-relative, posix-normalized.
    let rel = relative_path(canonical_root, file);
    let native_path = rel.clone();
    let logical_id = rel;

    let updated_at = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64);

    Ok(Some(DataItem {
        provider,
        kind: mapping.kind,
        logical_id,
        parent_id: None,
        native_path,
        content_hash: content_hash.clone(),
        updated_at,
        schema_fingerprint: None,
        merge_capability: mapping.capability,
        sensitivity: Sensitivity::WorkData,
        object_ids: vec![content_hash],
    }))
}

/// Compute a posix, root-relative path. Falls back to the file name if the file
/// is not under the (canonicalized) root.
fn relative_path(canonical_root: &Path, file: &Path) -> String {
    let canonical_file = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    match canonical_file.strip_prefix(canonical_root) {
        Ok(rel) => rel.to_string_lossy().replace('\\', "/"),
        Err(_) => file
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .replace('\\', "/"),
    }
}

/// All adapters for the given providers (installed or not — callers filter).
pub fn adapters_for(providers: &[WorkspaceProviderId]) -> Vec<Box<dyn ProviderAdapter>> {
    providers.iter().map(|p| adapter_for(*p)).collect()
}

/// Build the adapter for a single provider.
pub fn adapter_for(provider: WorkspaceProviderId) -> Box<dyn ProviderAdapter> {
    match provider {
        WorkspaceProviderId::Claude => Box::new(claude::adapter()),
        WorkspaceProviderId::Codex => Box::new(codex::adapter()),
        WorkspaceProviderId::GrokBuild => Box::new(grokbuild::adapter()),
        WorkspaceProviderId::OpenCode => Box::new(opencode::adapter()),
        WorkspaceProviderId::Cursor => Box::new(cursor::adapter()),
    }
}
