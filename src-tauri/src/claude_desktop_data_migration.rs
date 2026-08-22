//! Opt-in, reversible Claude Desktop 1P -> 3P local-data migration.
//!
//! # Why this exists
//!
//! On macOS, CC Switch keeps Claude Desktop's official (1P) and third-party (3P)
//! profiles in two separate application roots:
//!
//! ```text
//! ~/Library/Application Support/Claude
//! ~/Library/Application Support/Claude-3p
//! ```
//!
//! Applying a provider only rewrites deployment/profile configuration (see
//! [`crate::claude_desktop_config`]); it never moves the per-account data stored
//! under `claude-code-sessions`, `local-agent-mode-sessions`, the Scheduled /
//! Project / Artifact registries, and so on. After switching to a 3P provider,
//! Claude Desktop reads from the new `Claude-3p` root, so existing data in the
//! `Claude` root *appears* to vanish even though nothing was deleted.
//!
//! This module implements an explicit, user-confirmed migration that copies the
//! safe subset of that data into the 3P root. It follows the same safety model
//! as [`crate::codex_history_migration`]:
//!
//! - the source tree is always treated as read-only;
//! - existing target records are never overwritten (skip on conflict);
//! - the target account roots are backed up before any write;
//! - Cowork sessions are staged, verified, then installed atomically;
//! - every action is recorded in a ledger so restore can undo exactly what this
//!   migration installed and nothing created afterwards.
//!
//! Credentials, cookies, caches, VM bundles, OAuth state, and cloud Chat history
//! are intentionally out of scope and are never read or copied here.
//!
//! All core functions take explicit paths so they can be exercised against
//! temporary directories with synthetic fixtures; the Tauri commands in
//! [`crate::commands`] only resolve the default macOS roots and forward.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use chrono::Local;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::config::{copy_file, read_json_file};
use crate::error::AppError;

/// Glob-style prefix/suffix used for Claude Desktop session metadata files.
const LOCAL_META_PREFIX: &str = "local_";
const LOCAL_META_SUFFIX: &str = ".json";

/// Sub-directory (relative to an app root) holding Claude Code session indexes.
const CODE_CONTAINER: &str = "claude-code-sessions";
/// Sub-directory (relative to an app root) holding Cowork session metadata.
const COWORK_CONTAINER: &str = "local-agent-mode-sessions";

/// Backup namespace under `~/.cc-switch/backups`.
const MIGRATION_NAME: &str = "claude-desktop-data-migration";

/// Current ledger schema version.
const LEDGER_VERSION: u32 = 1;

/// Components this migration can copy. Anything else (credentials, caches,
/// VMs, cloud chat) is deliberately excluded.
pub const COMPONENT_CODE: &str = "code";
pub const COMPONENT_COWORK: &str = "cowork";
pub const COMPONENT_SCHEDULES: &str = "schedules";
pub const COMPONENT_PROJECTS: &str = "projects";
pub const COMPONENT_ARTIFACTS: &str = "artifacts";

const DEFAULT_COMPONENTS: [&str; 5] = [
    COMPONENT_CODE,
    COMPONENT_COWORK,
    COMPONENT_SCHEDULES,
    COMPONENT_PROJECTS,
    COMPONENT_ARTIFACTS,
];

/// Components that remain manual and are reported for transparency.
const MANUAL_COMPONENTS: [&str; 4] = [
    "cloud Chat conversations",
    "account preferences and credentials",
    "Connectors and OAuth grants",
    "Skill/Plugin packaging and runtime authentication",
];

// ---------------------------------------------------------------------------
// Public result types (serialized camelCase for the frontend)
// ---------------------------------------------------------------------------

/// Summary of one candidate Code/Cowork account root discovered during audit.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountRootCandidate {
    pub path: String,
    pub metadata_count: usize,
    pub scheduled_count: usize,
    pub invalid_json: usize,
    pub size_bytes: u64,
    pub folder_references: Vec<String>,
}

/// Audit of a single application root (1P or 3P).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppRootAudit {
    pub path: String,
    pub exists: bool,
    pub deployment_mode: Option<String>,
    pub size_bytes: u64,
    pub code_roots: Vec<AccountRootCandidate>,
    pub cowork_roots: Vec<AccountRootCandidate>,
}

/// Read-only inventory of the source and target roots. This is the first thing
/// the UI shows and never writes anything.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationAudit {
    pub supported: bool,
    pub source_app: AppRootAudit,
    pub target_app: AppRootAudit,
    pub shared_code_transcript_root: String,
    pub shared_code_transcript_count: usize,
    pub shared_documents_root: String,
    pub shared_documents_exist: bool,
}

/// A single user-approved OLD -> NEW absolute path-prefix mapping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PathMapping {
    pub old: String,
    pub new: String,
}

/// Per-component migration plan (Code or Cowork).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ComponentPlan {
    pub component: String,
    /// One of: ready | source-empty | ambiguous-source | ambiguous-target | missing-target-seed
    pub status: String,
    pub source_root: Option<String>,
    pub target_root: Option<String>,
    /// Candidate account roots when the choice is ambiguous (user must pick).
    pub candidates: Vec<String>,
    pub source_metadata: usize,
    pub target_metadata: usize,
    pub new_records: usize,
    pub conflicts: usize,
    pub invalid_json: usize,
    pub missing_session_directories: usize,
    pub scheduled_records: usize,
    pub missing_shared_transcripts: usize,
    pub missing_folder_paths: Vec<String>,
    pub estimated_copy_bytes: u64,
}

/// Full migration plan: the contract the user reviews and confirms before any
/// write happens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationPlan {
    pub source_app: String,
    pub target_app: String,
    pub code: ComponentPlan,
    pub cowork: ComponentPlan,
    pub path_maps: Vec<PathMapping>,
    pub estimated_copy_bytes: u64,
    pub estimated_backup_bytes: u64,
    /// Issues that must be resolved before apply is allowed (e.g. ambiguous
    /// account roots, missing target seed, invalid source JSON).
    pub blocking_issues: Vec<String>,
    pub manual_components: Vec<String>,
}

/// One installed / skipped / failed record in the migration ledger.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRecord {
    pub component: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ids: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_metadata: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// Fingerprint of the installed target, used to verify before restore.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    /// Fingerprint of the installed Cowork metadata file, verified separately
    /// before restore removes it so a user-updated task is never orphaned.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata_fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl MigrationRecord {
    fn new(component: &str, id: Option<String>) -> Self {
        Self {
            component: component.to_string(),
            id,
            ids: None,
            target_metadata: None,
            target_session: None,
            target: None,
            fingerprint: None,
            metadata_fingerprint: None,
            reason: None,
            error: None,
        }
    }
}

/// Durable record of what a migration did. Stored as JSON inside the backup
/// directory under `~/.cc-switch/backups` (CC Switch's own config area, never
/// inside Claude's account-owned configuration). Contains no transcript or
/// credential content — only IDs, paths, counts, and integrity fingerprints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationLedger {
    pub version: u32,
    pub created_at: String,
    pub source_app: String,
    pub target_app: String,
    pub backup_path: String,
    pub path_maps: Vec<PathMapping>,
    pub source_fingerprint: String,
    pub target_fingerprint: String,
    pub installed: Vec<MigrationRecord>,
    pub skipped: Vec<MigrationRecord>,
    pub failed: Vec<MigrationRecord>,
}

/// Result of an apply run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationApplyResult {
    pub backup_path: String,
    pub ledger_path: String,
    pub installed_count: usize,
    pub skipped_count: usize,
    pub failed_count: usize,
    pub failed: Vec<MigrationRecord>,
    /// Set when a later component failed after earlier components had already
    /// written records. The ledger is still persisted and the installed records
    /// remain restorable, so the UI must surface this rather than an opaque
    /// error that discards access to the ledger.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub apply_error: Option<String>,
}

/// One structural verification check.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyCheck {
    pub name: String,
    pub ok: bool,
    pub detail: String,
}

/// Result of a verify run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationVerifyResult {
    pub passed: bool,
    pub checks: Vec<VerifyCheck>,
}

/// Result of a restore run.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MigrationRestoreResult {
    pub backup_path: String,
    pub removed_count: usize,
    pub reverted_count: usize,
    pub kept_count: usize,
    /// Present when nothing was restored (e.g. no ledger found); the UI should
    /// surface this rather than reporting "restored 0 items" as success.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Resolved roots passed to the engine
// ---------------------------------------------------------------------------

/// Optional explicit account-root overrides supplied by the user when
/// auto-discovery finds multiple candidates (or none).
#[derive(Debug, Clone, Default)]
pub struct RootOverrides {
    pub source_code: Option<PathBuf>,
    pub target_code: Option<PathBuf>,
    pub source_cowork: Option<PathBuf>,
    pub target_cowork: Option<PathBuf>,
}

/// How a single component's account roots resolved.
#[derive(Debug, Clone)]
enum ComponentResolution {
    /// Source has no data for this component; nothing to migrate.
    Inactive,
    /// Both source and target resolved to a single account root.
    Ready { source: PathBuf, target: PathBuf },
    /// More than one source candidate and no override; user must choose.
    AmbiguousSource { candidates: Vec<PathBuf> },
    /// More than one target candidate and no override; user must choose.
    AmbiguousTarget { candidates: Vec<PathBuf> },
    /// Source has data but no target account root exists yet (no seed session).
    MissingTargetSeed { source: PathBuf },
}

// ---------------------------------------------------------------------------
// Default macOS root resolution (used by the Tauri commands)
// ---------------------------------------------------------------------------

/// Default 1P/3P application roots for the current platform.
///
/// Only macOS is resolved by default for v1; other platforms return `None` so
/// the caller can report "unsupported platform". The engine itself is
/// platform-agnostic and fully tested against explicit paths.
pub fn default_app_roots(home: &Path) -> Option<(PathBuf, PathBuf)> {
    if cfg!(target_os = "macos") {
        let base = home.join("Library").join("Application Support");
        Some((base.join("Claude"), base.join("Claude-3p")))
    } else {
        None
    }
}

/// Backup root for this migration under CC Switch's own config directory.
pub fn migration_backup_root() -> PathBuf {
    crate::config::get_app_config_dir()
        .join("backups")
        .join(MIGRATION_NAME)
}

// ---------------------------------------------------------------------------
// Small filesystem helpers
// ---------------------------------------------------------------------------

fn is_local_meta_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    name.starts_with(LOCAL_META_PREFIX) && name.ends_with(LOCAL_META_SUFFIX)
}

/// Metadata ID for a `local_<id>.json` file (its file stem).
fn metadata_id(path: &Path) -> Option<String> {
    path.file_stem()
        .and_then(|s| s.to_str())
        .map(str::to_string)
}

/// Direct `local_*.json` children of an account root (not recursive).
fn direct_meta_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return out;
    }
    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if is_local_meta_file(&path) {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

/// Recursively collect `local_*.json` files below `dir` into `out`.
fn collect_meta_files_recursive(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // Do not follow symlinks while discovering account roots.
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            collect_meta_files_recursive(&path, out);
        } else if is_local_meta_file(&path) {
            out.push(path);
        }
    }
}

/// Discover candidate account roots below `<app_root>/<container>`: every
/// directory that directly contains at least one `local_*.json` file.
fn find_account_roots(app_root: &Path, container: &str) -> Vec<PathBuf> {
    let container_dir = app_root.join(container);
    let mut files = Vec::new();
    collect_meta_files_recursive(&container_dir, &mut files);
    let mut roots: BTreeSet<PathBuf> = BTreeSet::new();
    for file in files {
        if let Some(parent) = file.parent() {
            roots.insert(parent.to_path_buf());
        }
    }
    roots.into_iter().collect()
}

/// Total size in bytes of a file or directory (symlinks counted by their own
/// metadata size, not followed).
fn path_size(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() || meta.file_type().is_symlink() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            total += path_size(&entry.path());
        }
    }
    total
}

/// SHA-256 hex digest of a file's contents.
fn sha256_file(path: &Path) -> Result<String, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::io(path, e))?;
    Ok(hex_digest(&bytes))
}

fn hex_digest(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Sorted multiset of per-file content hashes for every regular file under
/// `root`. Used to prove transcript/upload/output/audit payloads are
/// byte-identical before and after staging. Symlinks and directories are
/// excluded (directory renames do not change content).
fn content_hash_multiset(root: &Path) -> Vec<String> {
    let mut hashes = Vec::new();
    collect_content_hashes(root, &mut hashes);
    hashes.sort();
    hashes
}

fn collect_content_hashes(dir: &Path, out: &mut Vec<String>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        } else if meta.is_dir() {
            collect_content_hashes(&path, out);
        } else if meta.is_file() {
            if let Ok(hash) = sha256_file(&path) {
                out.push(hash);
            }
        }
    }
}

/// Fingerprint of a directory = hash over sorted
/// `relative_path:size:sha256` lines of every regular file. Includes the
/// content hash so a same-size edit after migration is still detected, and
/// restore refuses to delete a directory the user has since modified.
fn dir_fingerprint(root: &Path) -> String {
    let mut lines = Vec::new();
    collect_dir_listing(root, root, &mut lines);
    lines.sort();
    hex_digest(lines.join("\n").as_bytes())
}

fn collect_dir_listing(root: &Path, dir: &Path, out: &mut Vec<String>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = fs::symlink_metadata(&path) else {
            continue;
        };
        if meta.file_type().is_symlink() {
            continue;
        } else if meta.is_dir() {
            collect_dir_listing(root, &path, out);
        } else if meta.is_file() {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .to_string();
            let digest = sha256_file(&path).unwrap_or_else(|_| "err".to_string());
            out.push(format!("{rel}:{}:{digest}", meta.len()));
        }
    }
}

/// Recursively copy a directory tree, preserving symlinks where supported and
/// copying regular-file contents byte-for-byte. Never follows symlinks.
fn copy_dir_recursive(source: &Path, target: &Path) -> Result<(), AppError> {
    if !source.is_dir() {
        return Err(AppError::Config(format!(
            "源目录不存在: {}",
            source.display()
        )));
    }
    fs::create_dir_all(target).map_err(|e| AppError::io(target, e))?;
    let entries = fs::read_dir(source).map_err(|e| AppError::io(source, e))?;
    for entry in entries.flatten() {
        let src = entry.path();
        let dst = target.join(entry.file_name());
        let meta = fs::symlink_metadata(&src).map_err(|e| AppError::io(&src, e))?;
        if meta.file_type().is_symlink() {
            copy_symlink(&src, &dst)?;
        } else if meta.is_dir() {
            copy_dir_recursive(&src, &dst)?;
        } else if meta.is_file() {
            copy_file(&src, &dst)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    let link = fs::read_link(source).map_err(|e| AppError::io(source, e))?;
    if target.exists() || fs::symlink_metadata(target).is_ok() {
        return Err(AppError::Config(format!(
            "拒绝覆盖已存在的目标: {}",
            target.display()
        )));
    }
    std::os::unix::fs::symlink(&link, target).map_err(|e| AppError::io(target, e))
}

#[cfg(not(unix))]
fn copy_symlink(source: &Path, target: &Path) -> Result<(), AppError> {
    // On platforms where creating symlinks needs privilege, fall back to
    // copying the resolved content so payloads still migrate byte-for-byte.
    let resolved = source.canonicalize().map_err(|e| AppError::io(source, e))?;
    if resolved.is_dir() {
        copy_dir_recursive(&resolved, target)
    } else {
        copy_file(&resolved, target)
    }
}

/// Write `data` to `path` atomically, refusing to overwrite an existing file.
fn write_new_file_atomic(path: &Path, data: &[u8]) -> Result<(), AppError> {
    if path.exists() || fs::symlink_metadata(path).is_ok() {
        return Err(AppError::Config(format!(
            "拒绝覆盖已存在的目标: {}",
            path.display()
        )));
    }
    let parent = path
        .parent()
        .ok_or_else(|| AppError::Config("无效路径".to_string()))?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::Config("无效文件名".to_string()))?
        .to_string_lossy()
        .to_string();
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    let tmp = parent.join(format!(".{file_name}.migration-{nonce}"));
    fs::write(&tmp, data).map_err(|e| AppError::io(&tmp, e))?;
    // Best-effort flush to disk before the atomic rename.
    if let Ok(file) = fs::File::options().read(true).open(&tmp) {
        let _ = file.sync_all();
    }
    if let Err(e) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(AppError::io(path, e));
    }
    Ok(())
}

/// Copy a single file to a new location without overwriting.
fn copy_new_file(source: &Path, target: &Path) -> Result<(), AppError> {
    let bytes = fs::read(source).map_err(|e| AppError::io(source, e))?;
    write_new_file_atomic(target, &bytes)
}

/// Serialize a JSON value pretty + trailing newline (stable, no key sorting so
/// structural field order from the source is preserved).
fn json_bytes(value: &Value) -> Result<Vec<u8>, AppError> {
    let mut s =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    s.push('\n');
    Ok(s.into_bytes())
}

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    read_json_file::<Value>(path)
}

// ---------------------------------------------------------------------------
// Audit (read-only)
// ---------------------------------------------------------------------------

fn deployment_mode(app_root: &Path) -> Option<String> {
    let config = app_root.join("claude_desktop_config.json");
    if !config.is_file() {
        return None;
    }
    let value = read_json_value(&config).ok()?;
    value
        .get("deploymentMode")
        .and_then(Value::as_str)
        .map(str::to_string)
}

fn is_scheduled_meta(data: &Value) -> bool {
    data.get("scheduledTaskId").is_some()
        || data.get("sessionType").and_then(Value::as_str) == Some("scheduled")
}

fn account_root_summary(root: &Path) -> AccountRootCandidate {
    let files = direct_meta_files(root);
    let mut invalid = 0;
    let mut scheduled = 0;
    let mut folders: BTreeSet<String> = BTreeSet::new();
    for file in &files {
        match read_json_value(file) {
            Ok(data) => {
                if is_scheduled_meta(&data) {
                    scheduled += 1;
                }
                if let Some(arr) = data.get("userSelectedFolders").and_then(Value::as_array) {
                    for f in arr {
                        if let Some(s) = f.as_str() {
                            folders.insert(s.to_string());
                        }
                    }
                }
            }
            Err(_) => invalid += 1,
        }
    }
    AccountRootCandidate {
        path: root.display().to_string(),
        metadata_count: files.len(),
        scheduled_count: scheduled,
        invalid_json: invalid,
        size_bytes: path_size(root),
        folder_references: folders.into_iter().collect(),
    }
}

fn app_root_audit(app_root: &Path) -> AppRootAudit {
    let code_roots = find_account_roots(app_root, CODE_CONTAINER)
        .iter()
        .map(|p| account_root_summary(p))
        .collect();
    let cowork_roots = find_account_roots(app_root, COWORK_CONTAINER)
        .iter()
        .map(|p| account_root_summary(p))
        .collect();
    AppRootAudit {
        path: app_root.display().to_string(),
        exists: app_root.is_dir(),
        deployment_mode: deployment_mode(app_root),
        size_bytes: path_size(app_root),
        code_roots,
        cowork_roots,
    }
}

/// Stems (`cliSessionId`) of every shared Claude Code transcript under
/// `~/.claude/projects`.
fn transcript_stems(home: &Path) -> BTreeSet<String> {
    let root = home.join(".claude").join("projects");
    let mut stems = BTreeSet::new();
    collect_transcript_stems(&root, &mut stems);
    stems
}

fn collect_transcript_stems(dir: &Path, out: &mut BTreeSet<String>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            collect_transcript_stems(&path, out);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                out.insert(stem.to_string());
            }
        }
    }
}

/// Read-only inventory of source and target roots. Never writes anything.
pub fn build_audit(home: &Path, source_app: &Path, target_app: &Path) -> MigrationAudit {
    let stems = transcript_stems(home);
    let documents = home.join("Documents").join("Claude");
    MigrationAudit {
        supported: true,
        source_app: app_root_audit(source_app),
        target_app: app_root_audit(target_app),
        shared_code_transcript_root: home.join(".claude").join("projects").display().to_string(),
        shared_code_transcript_count: stems.len(),
        shared_documents_root: documents.display().to_string(),
        shared_documents_exist: documents.is_dir(),
    }
}

// ---------------------------------------------------------------------------
// Account-root resolution
// ---------------------------------------------------------------------------

fn resolve_component(
    source_app: &Path,
    target_app: &Path,
    container: &str,
    source_override: Option<&PathBuf>,
    target_override: Option<&PathBuf>,
) -> Result<ComponentResolution, AppError> {
    // Resolve source.
    let source = match source_override {
        Some(path) => {
            if !path.is_dir() {
                return Err(AppError::InvalidInput(format!(
                    "指定的源 {container} 账户根目录不存在: {}",
                    path.display()
                )));
            }
            Some(path.clone())
        }
        None => {
            let candidates = find_account_roots(source_app, container);
            match candidates.len() {
                0 => None,
                1 => Some(candidates.into_iter().next().expect("len == 1")),
                _ => return Ok(ComponentResolution::AmbiguousSource { candidates }),
            }
        }
    };

    let Some(source) = source else {
        return Ok(ComponentResolution::Inactive);
    };

    // Resolve target (only relevant when the source has data).
    match target_override {
        Some(path) => {
            if !path.is_dir() {
                return Err(AppError::InvalidInput(format!(
                    "指定的目标 {container} 账户根目录不存在: {}",
                    path.display()
                )));
            }
            Ok(ComponentResolution::Ready {
                source,
                target: path.clone(),
            })
        }
        None => {
            let candidates = find_account_roots(target_app, container);
            match candidates.len() {
                0 => Ok(ComponentResolution::MissingTargetSeed { source }),
                1 => Ok(ComponentResolution::Ready {
                    source,
                    target: candidates.into_iter().next().expect("len == 1"),
                }),
                _ => Ok(ComponentResolution::AmbiguousTarget { candidates }),
            }
        }
    }
}

/// Resolve all account roots, honoring user overrides. Returns an error only
/// for invalid explicit overrides; ambiguity and missing seeds are reported in
/// the plan so the UI can ask the user to choose.
fn resolve_roots(
    source_app: &Path,
    target_app: &Path,
    overrides: &RootOverrides,
) -> Result<ResolvedComponents, AppError> {
    let code = resolve_component(
        source_app,
        target_app,
        CODE_CONTAINER,
        overrides.source_code.as_ref(),
        overrides.target_code.as_ref(),
    )?;
    let cowork = resolve_component(
        source_app,
        target_app,
        COWORK_CONTAINER,
        overrides.source_cowork.as_ref(),
        overrides.target_cowork.as_ref(),
    )?;
    Ok(ResolvedComponents { code, cowork })
}

struct ResolvedComponents {
    code: ComponentResolution,
    cowork: ComponentResolution,
}

// ---------------------------------------------------------------------------
// Path mappings
// ---------------------------------------------------------------------------

/// Validate and normalize user-supplied path maps. Both sides must be absolute;
/// mappings are applied longest-prefix-first.
pub fn parse_path_maps(maps: &[PathMapping]) -> Result<Vec<(String, String)>, AppError> {
    let mut out: Vec<(String, String)> = Vec::new();
    for m in maps {
        let old = m.old.trim();
        let new = m.new.trim();
        if old.is_empty() || new.is_empty() {
            return Err(AppError::InvalidInput(
                "路径映射的 OLD 和 NEW 都不能为空".to_string(),
            ));
        }
        if !old.starts_with('/') || !new.starts_with('/') {
            return Err(AppError::InvalidInput(format!(
                "路径映射必须是绝对路径 (OLD=NEW): {old} -> {new}"
            )));
        }
        out.push((
            old.trim_end_matches('/').to_string(),
            new.trim_end_matches('/').to_string(),
        ));
    }
    out.sort_by_key(|b| std::cmp::Reverse(b.0.len()));
    Ok(out)
}

/// Replace `old` prefix with `new` at a path boundary only.
fn prefix_replace(value: &str, old: &str, new: &str) -> String {
    if value == old {
        return new.to_string();
    }
    if let Some(rest) = value.strip_prefix(&format!("{old}/")) {
        return format!("{new}/{rest}");
    }
    value.to_string()
}

/// Apply the first matching user path map to `value`.
fn apply_path_maps(value: &str, mappings: &[(String, String)]) -> String {
    for (old, new) in mappings {
        let replaced = prefix_replace(value, old, new);
        if replaced != value {
            return replaced;
        }
    }
    value.to_string()
}

// ---------------------------------------------------------------------------
// Plan (read-only)
// ---------------------------------------------------------------------------

fn plan_component(
    component: &str,
    resolution: &ComponentResolution,
    home: &Path,
    path_maps: &[(String, String)],
) -> ComponentPlan {
    let mut plan = ComponentPlan {
        component: component.to_string(),
        status: "ready".to_string(),
        source_root: None,
        target_root: None,
        candidates: Vec::new(),
        source_metadata: 0,
        target_metadata: 0,
        new_records: 0,
        conflicts: 0,
        invalid_json: 0,
        missing_session_directories: 0,
        scheduled_records: 0,
        missing_shared_transcripts: 0,
        missing_folder_paths: Vec::new(),
        estimated_copy_bytes: 0,
    };

    let (source, target) = match resolution {
        ComponentResolution::Inactive => {
            plan.status = "source-empty".to_string();
            return plan;
        }
        ComponentResolution::AmbiguousSource { candidates } => {
            plan.status = "ambiguous-source".to_string();
            plan.candidates = candidates.iter().map(|p| p.display().to_string()).collect();
            return plan;
        }
        ComponentResolution::AmbiguousTarget { candidates } => {
            plan.status = "ambiguous-target".to_string();
            plan.candidates = candidates.iter().map(|p| p.display().to_string()).collect();
            return plan;
        }
        ComponentResolution::MissingTargetSeed { source } => {
            plan.status = "missing-target-seed".to_string();
            plan.source_root = Some(source.display().to_string());
            plan.source_metadata = direct_meta_files(source).len();
            return plan;
        }
        ComponentResolution::Ready { source, target } => (source, target),
    };

    plan.source_root = Some(source.display().to_string());
    plan.target_root = Some(target.display().to_string());

    let source_files = direct_meta_files(source);
    plan.source_metadata = source_files.len();
    plan.target_metadata = direct_meta_files(target).len();

    let target_ids: HashSet<String> = direct_meta_files(target)
        .iter()
        .filter_map(|p| metadata_id(p))
        .collect();
    let known_stems = if component == COMPONENT_CODE {
        transcript_stems(home)
    } else {
        BTreeSet::new()
    };

    let mut missing_folders: BTreeSet<String> = BTreeSet::new();
    for meta in &source_files {
        let data = match read_json_value(meta) {
            Ok(d) => d,
            Err(_) => {
                plan.invalid_json += 1;
                continue;
            }
        };
        let id = metadata_id(meta).unwrap_or_default();
        if target_ids.contains(&id) {
            plan.conflicts += 1;
            continue;
        }
        plan.new_records += 1;
        plan.estimated_copy_bytes += path_size(meta);
        if component == COMPONENT_CODE {
            let cli_id = data.get("cliSessionId").and_then(Value::as_str);
            match cli_id {
                Some(id) if known_stems.contains(id) => {}
                _ => plan.missing_shared_transcripts += 1,
            }
        } else {
            if is_scheduled_meta(&data) {
                plan.scheduled_records += 1;
            }
            let session_dir = source.join(&id);
            if session_dir.is_dir() {
                plan.estimated_copy_bytes += path_size(&session_dir);
            } else {
                plan.missing_session_directories += 1;
            }
            if let Some(arr) = data.get("userSelectedFolders").and_then(Value::as_array) {
                for f in arr {
                    if let Some(s) = f.as_str() {
                        let mapped = apply_path_maps(s, path_maps);
                        if !Path::new(&mapped).is_dir() {
                            missing_folders.insert(mapped);
                        }
                    }
                }
            }
        }
    }
    plan.missing_folder_paths = missing_folders.into_iter().collect();
    plan
}

/// Build a read-only migration plan. Also reports blocking issues that must be
/// resolved before apply.
pub fn build_plan(
    home: &Path,
    source_app: &Path,
    target_app: &Path,
    overrides: &RootOverrides,
    path_maps: &[PathMapping],
) -> Result<MigrationPlan, AppError> {
    let resolved = resolve_roots(source_app, target_app, overrides)?;
    let maps = parse_path_maps(path_maps)?;

    let code = plan_component(COMPONENT_CODE, &resolved.code, home, &maps);
    let cowork = plan_component(COMPONENT_COWORK, &resolved.cowork, home, &maps);

    let mut blocking = Vec::new();
    for plan in [&code, &cowork] {
        let label = if plan.component == COMPONENT_CODE {
            "Code"
        } else {
            "Cowork"
        };
        match plan.status.as_str() {
            "ambiguous-source" => blocking.push(format!(
                "{label} 源存在多个候选账户根目录，请明确选择一个。"
            )),
            "ambiguous-target" => blocking.push(format!(
                "{label} 目标存在多个候选账户根目录，请明确选择一个。"
            )),
            "missing-target-seed" => blocking.push(format!(
                "{label} 目标还没有账户根目录。请先在 3P Claude Desktop 中创建一个新的{label}会话（种子会话），然后再迁移。"
            )),
            _ => {}
        }
        if plan.invalid_json > 0 {
            blocking.push(format!(
                "{label} 源包含 {} 个无法解析的 JSON 元数据文件。",
                plan.invalid_json
            ));
        }
    }

    // Estimated backup = current size of the target account roots we would
    // snapshot before writing.
    let mut backup_bytes = 0;
    if let Some(t) = &cowork.target_root {
        backup_bytes += path_size(Path::new(t));
    }
    if let Some(t) = &code.target_root {
        backup_bytes += path_size(Path::new(t));
    }

    Ok(MigrationPlan {
        source_app: source_app.display().to_string(),
        target_app: target_app.display().to_string(),
        estimated_copy_bytes: code.estimated_copy_bytes + cowork.estimated_copy_bytes,
        estimated_backup_bytes: backup_bytes,
        code,
        cowork,
        path_maps: path_maps.to_vec(),
        blocking_issues: blocking,
        manual_components: MANUAL_COMPONENTS.iter().map(|s| s.to_string()).collect(),
    })
}

// ---------------------------------------------------------------------------
// Cowork metadata transformation (structural fields only)
// ---------------------------------------------------------------------------

/// Encode a cwd the way Claude Desktop encodes project directories
/// (every non-alphanumeric byte becomes `-`).
fn encode_project_path(value: &str) -> String {
    value
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

fn transform_structural_path(
    value: &str,
    source_account: &Path,
    target_account: &Path,
    source_session: &Path,
    target_session: &Path,
    path_maps: &[(String, String)],
) -> String {
    let source_session = source_session.to_string_lossy().to_string();
    let target_session = target_session.to_string_lossy().to_string();
    let source_account = source_account.to_string_lossy().to_string();
    let target_account = target_account.to_string_lossy().to_string();

    let result = prefix_replace(value, &source_session, &target_session);
    let result = prefix_replace(&result, &source_account, &target_account);
    apply_path_maps(&result, path_maps)
}

/// Transform only the allowlisted structural path fields of a Cowork metadata
/// object. Never touches transcript/audit text or arbitrary strings.
fn transform_cowork_metadata(
    data: &Value,
    source_account: &Path,
    target_account: &Path,
    source_session: &Path,
    target_session: &Path,
    path_maps: &[(String, String)],
) -> Value {
    let mut result = data.clone();

    let map_value = |v: &Value| -> Value {
        match v.as_str() {
            Some(s) => Value::String(transform_structural_path(
                s,
                source_account,
                target_account,
                source_session,
                target_session,
                path_maps,
            )),
            None => v.clone(),
        }
    };

    if let Some(obj) = result.as_object_mut() {
        for key in ["cwd", "originCwd", "filePath"] {
            if let Some(v) = obj.get(key).cloned() {
                obj.insert(key.to_string(), map_value(&v));
            }
        }

        if let Some(Value::Array(folders)) = obj.get("userSelectedFolders").cloned() {
            let mapped: Vec<Value> = folders.iter().map(map_value).collect();
            obj.insert("userSelectedFolders".to_string(), Value::Array(mapped));
        }

        if let Some(Value::Array(files)) = obj.get("fsDetectedFiles").cloned() {
            let mapped: Vec<Value> = files
                .into_iter()
                .map(|mut rec| {
                    if let Some(rec_obj) = rec.as_object_mut() {
                        if let Some(host) = rec_obj.get("hostPath").cloned() {
                            rec_obj.insert("hostPath".to_string(), map_value(&host));
                        }
                    }
                    rec
                })
                .collect();
            obj.insert("fsDetectedFiles".to_string(), Value::Array(mapped));
        }

        // Rebuild resolvedFolderKinds only from folder paths that exist locally.
        // Whenever the record selects folders, always rewrite the kinds (even to
        // an empty list) so the result deterministically reflects which selected
        // folders actually exist — never a stale or absent value.
        let has_folders_array = obj.get("userSelectedFolders").is_some_and(Value::is_array);
        let existing: Vec<Value> = obj
            .get("userSelectedFolders")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|v| v.as_str().is_some_and(|s| Path::new(s).is_dir()))
            .map(|v| {
                let display = v.as_str().unwrap_or_default().to_string();
                let mut kind = Map::new();
                kind.insert("display".to_string(), Value::String(display));
                kind.insert("kind".to_string(), Value::String("local".to_string()));
                Value::Object(kind)
            })
            .collect();
        if has_folders_array || obj.contains_key("resolvedFolderKinds") || !existing.is_empty() {
            obj.insert("resolvedFolderKinds".to_string(), Value::Array(existing));
        }
    }

    result
}

/// Rename the embedded per-session transcript project directory to match the
/// transformed cwd, so the copied session still finds its transcript. Only
/// renames a uniquely-identified directory; never overwrites.
fn adjust_embedded_transcript_dir(staged_session: &Path, metadata: &Value) -> Result<(), AppError> {
    let cwd = metadata.get("cwd").and_then(Value::as_str);
    let cli_id = metadata.get("cliSessionId").and_then(Value::as_str);
    let (Some(cwd), Some(cli_id)) = (cwd, cli_id) else {
        return Ok(());
    };
    let projects = staged_session.join(".claude").join("projects");
    if !projects.is_dir() {
        return Ok(());
    }
    let file_name = format!("{cli_id}.jsonl");
    let mut matches: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&projects) {
        for entry in entries.flatten() {
            let candidate = entry.path().join(&file_name);
            if entry.path().is_dir() && candidate.is_file() {
                matches.push(entry.path());
            }
        }
    }
    if matches.len() != 1 {
        return Ok(());
    }
    let current_dir = &matches[0];
    let expected_dir = projects.join(encode_project_path(cwd));
    if current_dir == &expected_dir {
        return Ok(());
    }
    if expected_dir.exists() {
        return Err(AppError::Config(format!(
            "无法重命名内嵌 transcript 目录，目标已存在: {}",
            expected_dir.display()
        )));
    }
    fs::rename(current_dir, &expected_dir).map_err(|e| AppError::io(&expected_dir, e))
}

// ---------------------------------------------------------------------------
// Backup
// ---------------------------------------------------------------------------

fn backup_target_roots(
    target_code: Option<&Path>,
    target_cowork: Option<&Path>,
    backup_parent: &Path,
) -> Result<PathBuf, AppError> {
    fs::create_dir_all(backup_parent).map_err(|e| AppError::io(backup_parent, e))?;
    let stamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    let nonce = &uuid::Uuid::new_v4().simple().to_string()[..6];
    let backup_dir = backup_parent.join(format!("claude-3p-before-migration-{stamp}-{nonce}"));
    if backup_dir.exists() {
        return Err(AppError::Config(format!(
            "备份目录已存在: {}",
            backup_dir.display()
        )));
    }
    fs::create_dir_all(&backup_dir).map_err(|e| AppError::io(&backup_dir, e))?;
    if let Some(code) = target_code {
        if code.is_dir() {
            copy_dir_recursive(code, &backup_dir.join("claude-code-account"))?;
        }
    }
    if let Some(cowork) = target_cowork {
        if cowork.is_dir() {
            copy_dir_recursive(cowork, &backup_dir.join("cowork-account"))?;
        }
    }
    Ok(backup_dir)
}

// ---------------------------------------------------------------------------
// Installers (append to the ledger)
// ---------------------------------------------------------------------------

fn fingerprint_file(path: &Path) -> Option<String> {
    sha256_file(path).ok().map(|h| format!("sha256:{h}"))
}

fn install_code_indexes(source: &Path, target: &Path, ledger: &mut MigrationLedger) {
    let target_ids: HashSet<String> = direct_meta_files(target)
        .iter()
        .filter_map(|p| metadata_id(p))
        .collect();
    for source_meta in direct_meta_files(source) {
        let id = metadata_id(&source_meta).unwrap_or_default();
        let mut record = MigrationRecord::new(COMPONENT_CODE, Some(id.clone()));
        if target_ids.contains(&id) {
            record.reason = Some("target-exists".to_string());
            ledger.skipped.push(record);
            continue;
        }
        let target_meta = target.join(source_meta.file_name().expect("is file"));
        let result = (|| -> Result<(), AppError> {
            // Validate source JSON before copying.
            read_json_value(&source_meta)?;
            copy_new_file(&source_meta, &target_meta)?;
            if sha256_file(&source_meta)? != sha256_file(&target_meta)? {
                return Err(AppError::Message(
                    "复制后哈希不一致 (code index)".to_string(),
                ));
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                record.target_metadata = Some(target_meta.display().to_string());
                record.fingerprint = fingerprint_file(&target_meta);
                ledger.installed.push(record);
            }
            Err(e) => {
                record.error = Some(e.to_string());
                ledger.failed.push(record);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn install_cowork_sessions(
    source: &Path,
    target: &Path,
    path_maps: &[(String, String)],
    ledger: &mut MigrationLedger,
) {
    let target_ids: HashSet<String> = direct_meta_files(target)
        .iter()
        .filter_map(|p| metadata_id(p))
        .collect();
    for source_meta in direct_meta_files(source) {
        let id = metadata_id(&source_meta).unwrap_or_default();
        let mut record = MigrationRecord::new(COMPONENT_COWORK, Some(id.clone()));
        let target_meta = target.join(source_meta.file_name().expect("is file"));
        let source_session = source.join(&id);
        let target_session = target.join(&id);

        if target_ids.contains(&id) || target_meta.exists() || target_session.exists() {
            record.reason = Some("target-conflict".to_string());
            ledger.skipped.push(record);
            continue;
        }
        if !source_session.is_dir() {
            record.error = Some("源会话目录缺失（元数据没有同名会话目录）".to_string());
            ledger.failed.push(record);
            continue;
        }

        let nonce = uuid::Uuid::new_v4().simple().to_string();
        let stage_session = target.join(format!(".{id}.migration-{nonce}"));

        let result = (|| -> Result<Value, AppError> {
            let raw = read_json_value(&source_meta)?;
            if !raw.is_object() {
                return Err(AppError::Message("元数据根节点不是 JSON 对象".to_string()));
            }
            let transformed = transform_cowork_metadata(
                &raw,
                source,
                target,
                &source_session,
                &target_session,
                path_maps,
            );
            let before_hashes = content_hash_multiset(&source_session);
            copy_dir_recursive(&source_session, &stage_session)?;
            adjust_embedded_transcript_dir(&stage_session, &transformed)?;
            let after_hashes = content_hash_multiset(&stage_session);
            if before_hashes != after_hashes {
                return Err(AppError::Message(
                    "staging 后会话负载哈希发生变化".to_string(),
                ));
            }
            // Install the session directory first (atomic rename on one volume).
            fs::rename(&stage_session, &target_session)
                .map_err(|e| AppError::io(&target_session, e))?;
            // Then install the metadata file last. If this fails, roll back the
            // freshly-installed session directory so we never leave an unpaired
            // directory behind.
            if let Err(e) = write_new_file_atomic(&target_meta, &json_bytes(&transformed)?) {
                let _ = fs::remove_dir_all(&target_session);
                return Err(e);
            }
            Ok(transformed)
        })();

        match result {
            Ok(_transformed) => {
                record.target_metadata = Some(target_meta.display().to_string());
                record.target_session = Some(target_session.display().to_string());
                record.fingerprint = Some(dir_fingerprint(&target_session));
                record.metadata_fingerprint = fingerprint_file(&target_meta);
                ledger.installed.push(record);
            }
            Err(e) => {
                if stage_session.exists() {
                    let _ = fs::remove_dir_all(&stage_session);
                }
                record.error = Some(e.to_string());
                ledger.failed.push(record);
            }
        }
    }
}

fn merge_scheduled_tasks(
    source: &Path,
    target: &Path,
    path_maps: &[(String, String)],
    ledger: &mut MigrationLedger,
) -> Result<(), AppError> {
    let source_path = source.join("scheduled-tasks.json");
    let target_path = target.join("scheduled-tasks.json");
    if !source_path.is_file() {
        let mut r = MigrationRecord::new(COMPONENT_SCHEDULES, None);
        r.reason = Some("source-definition-missing".to_string());
        ledger.skipped.push(r);
        return Ok(());
    }
    let source_data = read_json_value(&source_path)?;
    let mut target_data = if target_path.is_file() {
        read_json_value(&target_path)?
    } else {
        Value::Object(Map::new())
    };
    if !source_data.is_object() || !target_data.is_object() {
        return Err(AppError::Message(
            "Scheduled 任务注册表必须是 JSON 对象".to_string(),
        ));
    }
    let empty: Vec<Value> = Vec::new();
    let source_tasks = source_data
        .get("scheduledTasks")
        .and_then(Value::as_array)
        .unwrap_or(&empty);

    let target_obj = target_data.as_object_mut().expect("checked object");
    if !target_obj.contains_key("scheduledTasks") {
        target_obj.insert("scheduledTasks".to_string(), Value::Array(Vec::new()));
    }
    let target_tasks = target_obj
        .get_mut("scheduledTasks")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| AppError::Message("目标 scheduledTasks 不是数组".to_string()))?;

    let mut existing_ids: HashSet<String> = target_tasks
        .iter()
        .filter_map(|t| t.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();

    let mut added: Vec<String> = Vec::new();
    for source_task in source_tasks {
        let Some(task_id) = source_task.get("id").and_then(Value::as_str) else {
            continue;
        };
        if existing_ids.contains(task_id) {
            continue;
        }
        let mut task = source_task.clone();
        if let Some(obj) = task.as_object_mut() {
            // Imported Scheduled definitions stay disabled until the user
            // reviews model, folders, permissions, and timing.
            obj.insert("enabled".to_string(), Value::Bool(false));
            if let Some(fp) = obj.get("filePath").cloned() {
                if let Some(s) = fp.as_str() {
                    obj.insert(
                        "filePath".to_string(),
                        Value::String(apply_path_maps(s, path_maps)),
                    );
                }
            }
            if let Some(Value::Array(folders)) = obj.get("userSelectedFolders").cloned() {
                let mapped: Vec<Value> = folders
                    .into_iter()
                    .map(|v| match v.as_str() {
                        Some(s) => Value::String(apply_path_maps(s, path_maps)),
                        None => v.clone(),
                    })
                    .collect();
                obj.insert("userSelectedFolders".to_string(), Value::Array(mapped));
            }
        }
        target_tasks.push(task);
        existing_ids.insert(task_id.to_string());
        added.push(task_id.to_string());
    }

    if added.is_empty() {
        let mut r = MigrationRecord::new(COMPONENT_SCHEDULES, None);
        r.reason = Some("no-new-definitions".to_string());
        ledger.skipped.push(r);
        return Ok(());
    }

    // The enclosing target root was already backed up, so an atomic rewrite of
    // the merged registry is safe here.
    let bytes = json_bytes(&target_data)?;
    crate::config::atomic_write(&target_path, &bytes)?;
    let mut r = MigrationRecord::new(COMPONENT_SCHEDULES, None);
    r.ids = Some(added);
    r.target = Some(target_path.display().to_string());
    r.fingerprint = fingerprint_file(&target_path);
    ledger.installed.push(r);
    Ok(())
}

fn copy_missing_project_cache(
    source: &Path,
    target: &Path,
    ledger: &mut MigrationLedger,
) -> Result<(), AppError> {
    let source_cache = source.join(".project-cache");
    let target_cache = target.join(".project-cache");
    if !source_cache.is_dir() {
        let mut r = MigrationRecord::new(COMPONENT_PROJECTS, None);
        r.reason = Some("source-cache-missing".to_string());
        ledger.skipped.push(r);
        return Ok(());
    }
    fs::create_dir_all(&target_cache).map_err(|e| AppError::io(&target_cache, e))?;
    let mut projects: Vec<PathBuf> = Vec::new();
    if let Ok(entries) = fs::read_dir(&source_cache) {
        for entry in entries.flatten() {
            if entry.path().is_dir() {
                projects.push(entry.path());
            }
        }
    }
    projects.sort();
    for project in projects {
        let name = project
            .file_name()
            .expect("is dir")
            .to_string_lossy()
            .to_string();
        let target_project = target_cache.join(&name);
        let mut record = MigrationRecord::new(COMPONENT_PROJECTS, Some(name));
        if target_project.exists() {
            record.reason = Some("target-exists".to_string());
            ledger.skipped.push(record);
            continue;
        }
        match copy_dir_recursive(&project, &target_project) {
            Ok(()) => {
                record.target = Some(target_project.display().to_string());
                record.fingerprint = Some(dir_fingerprint(&target_project));
                ledger.installed.push(record);
            }
            Err(e) => {
                record.error = Some(e.to_string());
                ledger.failed.push(record);
            }
        }
    }
    Ok(())
}

fn merge_artifacts(
    source: &Path,
    target: &Path,
    home: &Path,
    ledger: &mut MigrationLedger,
) -> Result<(), AppError> {
    let source_registry = source.join("artifacts.json");
    let target_registry = target.join("artifacts.json");
    if !source_registry.is_file() {
        let mut r = MigrationRecord::new(COMPONENT_ARTIFACTS, None);
        r.reason = Some("source-registry-missing".to_string());
        ledger.skipped.push(r);
        return Ok(());
    }
    let source_data = read_json_value(&source_registry)?;
    let source_arr = source_data
        .as_array()
        .ok_or_else(|| AppError::Message("源 artifacts 注册表必须是 JSON 数组".to_string()))?;
    let mut target_data = if target_registry.is_file() {
        read_json_value(&target_registry)?
    } else {
        Value::Array(Vec::new())
    };
    if !target_data.is_array() {
        return Err(AppError::Message(
            "目标 artifacts 注册表必须是 JSON 数组".to_string(),
        ));
    }
    let existing_ids: HashSet<String> = target_data
        .as_array()
        .expect("checked array")
        .iter()
        .filter_map(|i| i.get("id").and_then(Value::as_str).map(str::to_string))
        .collect();

    let entity_root = home.join("Documents").join("Claude").join("Artifacts");
    let mut valid: Vec<Value> = Vec::new();
    let mut added_ids: Vec<String> = Vec::new();
    let mut missing: Vec<String> = Vec::new();
    for item in source_arr {
        let Some(artifact_id) = item.get("id").and_then(Value::as_str) else {
            continue;
        };
        if existing_ids.contains(artifact_id) {
            let mut r = MigrationRecord::new(COMPONENT_ARTIFACTS, Some(artifact_id.to_string()));
            r.reason = Some("target-exists".to_string());
            ledger.skipped.push(r);
            continue;
        }
        if entity_root.join(artifact_id).is_dir() {
            valid.push(item.clone());
            added_ids.push(artifact_id.to_string());
        } else {
            missing.push(artifact_id.to_string());
        }
    }

    if !valid.is_empty() {
        let target_arr = target_data.as_array_mut().expect("checked array");
        target_arr.extend(valid);
        let bytes = json_bytes(&target_data)?;
        crate::config::atomic_write(&target_registry, &bytes)?;
        let mut r = MigrationRecord::new(COMPONENT_ARTIFACTS, None);
        r.ids = Some(added_ids);
        r.target = Some(target_registry.display().to_string());
        r.fingerprint = fingerprint_file(&target_registry);
        ledger.installed.push(r);
    }
    for artifact_id in missing {
        let mut r = MigrationRecord::new(COMPONENT_ARTIFACTS, Some(artifact_id));
        r.reason = Some("shared-entity-missing".to_string());
        ledger.skipped.push(r);
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Apply
// ---------------------------------------------------------------------------

/// Best-effort check for a running Claude Desktop process. Only enforced on
/// macOS (the v1 target). Returns false when the check cannot run.
pub fn claude_desktop_process_running() -> bool {
    if !cfg!(target_os = "macos") {
        return false;
    }
    std::process::Command::new("pgrep")
        .args(["-x", "Claude"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Apply a reviewed plan. `claude_running` is supplied by the caller so this
/// stays testable; the command layer computes it via
/// [`claude_desktop_process_running`].
#[allow(clippy::too_many_arguments)]
pub fn apply_migration(
    home: &Path,
    source_app: &Path,
    target_app: &Path,
    overrides: &RootOverrides,
    path_maps: &[PathMapping],
    components: &[String],
    backup_parent: &Path,
    claude_running: bool,
) -> Result<MigrationApplyResult, AppError> {
    if claude_running {
        return Err(AppError::localized(
            "claude_desktop.migration_app_running",
            "Claude Desktop 正在运行。请先完全退出 Claude Desktop，再执行迁移。",
            "Claude Desktop is running. Quit it fully before applying the migration.",
        ));
    }

    let plan = build_plan(home, source_app, target_app, overrides, path_maps)?;
    if !plan.blocking_issues.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "迁移前必须先解决以下问题:\n- {}",
            plan.blocking_issues.join("\n- ")
        )));
    }

    // Validate component selection.
    let selected: BTreeSet<String> = if components.is_empty() {
        DEFAULT_COMPONENTS.iter().map(|s| s.to_string()).collect()
    } else {
        components.iter().cloned().collect()
    };
    let known: BTreeSet<String> = DEFAULT_COMPONENTS.iter().map(|s| s.to_string()).collect();
    let unknown: Vec<String> = selected.difference(&known).cloned().collect();
    if !unknown.is_empty() {
        return Err(AppError::InvalidInput(format!(
            "未知组件: {}",
            unknown.join(", ")
        )));
    }

    let resolved = resolve_roots(source_app, target_app, overrides)?;
    let maps = parse_path_maps(path_maps)?;

    // Resolve code/cowork roots (both must be Ready if selected and active).
    let (code_source, code_target) = match &resolved.code {
        ComponentResolution::Ready { source, target } => {
            (Some(source.clone()), Some(target.clone()))
        }
        ComponentResolution::Inactive => (None, None),
        _ => {
            return Err(AppError::InvalidInput(
                "Code 账户根目录尚未就绪（存在歧义或缺少种子会话）".to_string(),
            ))
        }
    };
    let (cowork_source, cowork_target) = match &resolved.cowork {
        ComponentResolution::Ready { source, target } => {
            (Some(source.clone()), Some(target.clone()))
        }
        ComponentResolution::Inactive => (None, None),
        _ => {
            return Err(AppError::InvalidInput(
                "Cowork 账户根目录尚未就绪（存在歧义或缺少种子会话）".to_string(),
            ))
        }
    };

    // Back up the target account roots before any write.
    let backup_dir = backup_target_roots(
        code_target.as_deref(),
        cowork_target.as_deref(),
        backup_parent,
    )?;

    let source_fingerprint =
        compute_roots_fingerprint(code_source.as_deref(), cowork_source.as_deref());
    let target_fingerprint =
        compute_roots_fingerprint(code_target.as_deref(), cowork_target.as_deref());

    let mut ledger = MigrationLedger {
        version: LEDGER_VERSION,
        created_at: Local::now().to_rfc3339(),
        source_app: source_app.display().to_string(),
        target_app: target_app.display().to_string(),
        backup_path: backup_dir.display().to_string(),
        path_maps: path_maps.to_vec(),
        source_fingerprint,
        target_fingerprint,
        installed: Vec::new(),
        skipped: Vec::new(),
        failed: Vec::new(),
    };

    // Best-effort apply; always persist the ledger even on partial failure.
    let apply_result = (|| -> Result<(), AppError> {
        if selected.contains(COMPONENT_CODE) {
            if let (Some(src), Some(tgt)) = (&code_source, &code_target) {
                install_code_indexes(src, tgt, &mut ledger);
            }
        }
        if selected.contains(COMPONENT_COWORK) {
            if let (Some(src), Some(tgt)) = (&cowork_source, &cowork_target) {
                install_cowork_sessions(src, tgt, &maps, &mut ledger);
            }
        }
        if selected.contains(COMPONENT_SCHEDULES) {
            if let (Some(src), Some(tgt)) = (&cowork_source, &cowork_target) {
                merge_scheduled_tasks(src, tgt, &maps, &mut ledger)?;
            }
        }
        if selected.contains(COMPONENT_PROJECTS) {
            if let (Some(src), Some(tgt)) = (&cowork_source, &cowork_target) {
                copy_missing_project_cache(src, tgt, &mut ledger)?;
            }
        }
        if selected.contains(COMPONENT_ARTIFACTS) {
            if let (Some(src), Some(tgt)) = (&cowork_source, &cowork_target) {
                merge_artifacts(src, tgt, home, &mut ledger)?;
            }
        }
        Ok(())
    })();

    // Persist the plan + ledger into the backup directory regardless of outcome.
    let ledger_path = backup_dir.join("migration-ledger.json");
    let plan_path = backup_dir.join("plan.json");
    let ledger_json = json_bytes(
        &serde_json::to_value(&ledger).map_err(|e| AppError::JsonSerialize { source: e })?,
    )?;
    let plan_json = json_bytes(
        &serde_json::to_value(&plan).map_err(|e| AppError::JsonSerialize { source: e })?,
    )?;
    // The ledger must survive even if apply failed partway.
    let _ = write_new_file_atomic(&plan_path, &plan_json);
    write_new_file_atomic(&ledger_path, &ledger_json)?;

    // A later component failing must not discard access to the ledger: earlier
    // Code/Cowork records may already be installed, and the user needs Undo to
    // target this exact ledger (a retry would otherwise write a newer
    // skip-only ledger that latest-ledger discovery would prefer).
    let apply_error = match apply_result {
        Ok(()) => None,
        Err(e) => Some(e.to_string()),
    };

    Ok(MigrationApplyResult {
        backup_path: backup_dir.display().to_string(),
        ledger_path: ledger_path.display().to_string(),
        installed_count: ledger.installed.len(),
        skipped_count: ledger.skipped.len(),
        failed_count: ledger.failed.len(),
        failed: ledger.failed.clone(),
        apply_error,
    })
}

/// Fingerprint over a set of account roots = hash of sorted `path:size` lines
/// for the metadata files directly inside them.
fn compute_roots_fingerprint(code: Option<&Path>, cowork: Option<&Path>) -> String {
    let mut lines = Vec::new();
    for root in [code, cowork].into_iter().flatten() {
        for meta in direct_meta_files(root) {
            let size = fs::metadata(&meta).map(|m| m.len()).unwrap_or(0);
            lines.push(format!("{}:{size}", meta.display()));
        }
    }
    lines.sort();
    hex_digest(lines.join("\n").as_bytes())
}

// ---------------------------------------------------------------------------
// Verify (read-only)
// ---------------------------------------------------------------------------

/// Structural verification of the target after a migration. Read-only.
///
/// `components` restricts verification to the components that were applied
/// (empty = all). Without this, a successful partial migration would report the
/// intentionally-uninstalled source records as missing.
pub fn verify_migration(
    home: &Path,
    source_app: &Path,
    target_app: &Path,
    overrides: &RootOverrides,
    components: &[String],
) -> Result<MigrationVerifyResult, AppError> {
    let resolved = resolve_roots(source_app, target_app, overrides)?;
    let selected: BTreeSet<String> = if components.is_empty() {
        DEFAULT_COMPONENTS.iter().map(|s| s.to_string()).collect()
    } else {
        components.iter().cloned().collect()
    };
    let mut checks: Vec<VerifyCheck> = Vec::new();

    for (component, resolution) in [
        (COMPONENT_CODE, &resolved.code),
        (COMPONENT_COWORK, &resolved.cowork),
    ] {
        if !selected.contains(component) {
            continue;
        }
        let ComponentResolution::Ready { source, target } = resolution else {
            continue;
        };
        let source_ids: BTreeSet<String> = direct_meta_files(source)
            .iter()
            .filter_map(|p| metadata_id(p))
            .collect();
        let target_ids: BTreeSet<String> = direct_meta_files(target)
            .iter()
            .filter_map(|p| metadata_id(p))
            .collect();
        let missing: Vec<String> = source_ids.difference(&target_ids).cloned().collect();
        checks.push(VerifyCheck {
            name: format!("{component}: source ids present in target"),
            ok: missing.is_empty(),
            detail: if missing.is_empty() {
                format!("{} 条源记录全部存在", source_ids.len())
            } else {
                format!("缺失 {} 条: {}", missing.len(), missing.join(", "))
            },
        });

        if component == COMPONENT_COWORK {
            let unpaired: Vec<String> = direct_meta_files(target)
                .iter()
                .filter_map(|p| {
                    let id = metadata_id(p)?;
                    if target.join(&id).is_dir() {
                        None
                    } else {
                        Some(id)
                    }
                })
                .collect();
            checks.push(VerifyCheck {
                name: "cowork: metadata paired with session dir".to_string(),
                ok: unpaired.is_empty(),
                detail: if unpaired.is_empty() {
                    "所有 Cowork 元数据都有同名会话目录".to_string()
                } else {
                    format!("{} 条缺少会话目录: {}", unpaired.len(), unpaired.join(", "))
                },
            });
        }

        if component == COMPONENT_CODE {
            let stems = transcript_stems(home);
            let mut unresolved = 0;
            for meta in direct_meta_files(target) {
                if let Ok(data) = read_json_value(&meta) {
                    let cli_id = data.get("cliSessionId").and_then(Value::as_str);
                    match cli_id {
                        Some(id) if stems.contains(id) => {}
                        _ => unresolved += 1,
                    }
                }
            }
            checks.push(VerifyCheck {
                name: "code: cliSessionId resolves to shared transcript".to_string(),
                ok: unresolved == 0,
                detail: if unresolved == 0 {
                    "所有 Code 索引都能解析到共享 transcript".to_string()
                } else {
                    format!("{unresolved} 条索引缺少共享 transcript")
                },
            });
        }
    }

    // Imported Scheduled definitions must remain disabled.
    if selected.contains(COMPONENT_SCHEDULES) {
        if let ComponentResolution::Ready { target, .. } = &resolved.cowork {
            let scheduled_path = target.join("scheduled-tasks.json");
            let mut enabled = 0;
            if scheduled_path.is_file() {
                if let Ok(data) = read_json_value(&scheduled_path) {
                    if let Some(tasks) = data.get("scheduledTasks").and_then(Value::as_array) {
                        for task in tasks {
                            if task.get("enabled").and_then(Value::as_bool) == Some(true) {
                                enabled += 1;
                            }
                        }
                    }
                }
            }
            checks.push(VerifyCheck {
                name: "schedules: imported definitions remain disabled".to_string(),
                ok: enabled == 0,
                detail: if enabled == 0 {
                    "没有处于启用状态的 Scheduled 定义".to_string()
                } else {
                    format!("{enabled} 条 Scheduled 定义处于启用状态，需要人工检查")
                },
            });
        }
    }

    let passed = checks.iter().all(|c| c.ok);
    Ok(MigrationVerifyResult { passed, checks })
}

// ---------------------------------------------------------------------------
// Restore (ledger-driven, surgical)
// ---------------------------------------------------------------------------

/// Locate the most recent ledger below a backup parent directory.
pub fn latest_ledger_path(backup_parent: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    collect_ledgers(backup_parent, &mut candidates);
    candidates.sort();
    candidates.pop()
}

fn collect_ledgers(dir: &Path, out: &mut Vec<PathBuf>) {
    if !dir.is_dir() {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let ledger = path.join("migration-ledger.json");
            if ledger.is_file() {
                out.push(ledger);
            } else {
                collect_ledgers(&path, out);
            }
        }
    }
}

fn load_ledger(ledger_path: &Path) -> Result<MigrationLedger, AppError> {
    let ledger: MigrationLedger = read_json_file(ledger_path)?;
    if ledger.version != LEDGER_VERSION {
        return Err(AppError::InvalidInput(format!(
            "不支持的迁移账本版本: {} (期望 {LEDGER_VERSION})",
            ledger.version
        )));
    }
    Ok(ledger)
}

fn remove_file_if_matches(path: &Path, fingerprint: Option<&String>) -> Result<bool, AppError> {
    if !path.exists() {
        return Ok(false);
    }
    // Verify the installed file is byte-identical to what we wrote; if the user
    // modified it after migration, keep it rather than deleting their edits.
    if let Some(fp) = fingerprint {
        if fingerprint_file(path).as_ref() != Some(fp) {
            return Ok(false);
        }
    }
    fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
    Ok(true)
}

fn remove_dir_if_matches(path: &Path, fingerprint: Option<&String>) -> Result<bool, AppError> {
    if !path.is_dir() {
        return Ok(false);
    }
    if let Some(fp) = fingerprint {
        if &dir_fingerprint(path) != fp {
            return Ok(false);
        }
    }
    fs::remove_dir_all(path).map_err(|e| AppError::io(path, e))?;
    Ok(true)
}

/// True when `path` still matches the recorded fingerprint. A missing
/// fingerprint means "not verifiable", which is treated as touched so restore
/// never deletes something it cannot verify.
fn file_untouched(path: &Path, fingerprint: Option<&String>) -> bool {
    match fingerprint {
        Some(fp) => fingerprint_file(path).as_ref() == Some(fp),
        None => false,
    }
}

/// Same as [`file_untouched`] for a directory fingerprint.
fn dir_untouched(path: &Path, fingerprint: Option<&String>) -> bool {
    match fingerprint {
        Some(fp) => &dir_fingerprint(path) == fp,
        None => false,
    }
}

/// Undo exactly what a migration installed, driven by its ledger. Sessions or
/// registry entries created *after* the migration are never touched.
///
/// `ledger_path` points at a specific `migration-ledger.json`; when `None`,
/// the most recent ledger below `backup_parent` is used.
pub fn restore_migration(
    ledger_path: Option<&Path>,
    backup_parent: &Path,
) -> Result<MigrationRestoreResult, AppError> {
    let ledger_path = match ledger_path {
        Some(p) => p.to_path_buf(),
        None => match latest_ledger_path(backup_parent) {
            Some(p) => p,
            None => {
                return Ok(MigrationRestoreResult {
                    backup_path: backup_parent.display().to_string(),
                    removed_count: 0,
                    reverted_count: 0,
                    kept_count: 0,
                    skipped_reason: Some("no-ledger".to_string()),
                    notes: None,
                })
            }
        },
    };
    let ledger = load_ledger(&ledger_path)?;
    let backup_path = ledger.backup_path.clone();

    let mut removed = 0;
    let mut reverted = 0;
    let mut kept = 0;
    let mut notes: Vec<String> = Vec::new();

    for record in &ledger.installed {
        match record.component.as_str() {
            COMPONENT_CODE => {
                if let Some(meta) = &record.target_metadata {
                    match remove_file_if_matches(Path::new(meta), record.fingerprint.as_ref()) {
                        Ok(true) => removed += 1,
                        Ok(false) => {
                            kept += 1;
                            notes.push(format!(
                                "保留 {}（已不存在或已被修改）",
                                record.id.clone().unwrap_or_default()
                            ));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            COMPONENT_COWORK => {
                // A Cowork task is metadata + session dir. Undo a task only when
                // both halves still match what was installed; if the user
                // continued/renamed/updated either half after migration, the
                // whole pair is kept so the task is never orphaned or split.
                let meta_path = record.target_metadata.as_ref().map(Path::new);
                let session_path = record.target_session.as_ref().map(Path::new);
                let meta_ok = meta_path
                    .is_some_and(|p| file_untouched(p, record.metadata_fingerprint.as_ref()));
                let session_ok =
                    session_path.is_some_and(|p| dir_untouched(p, record.fingerprint.as_ref()));
                if meta_ok && session_ok {
                    if let Some(meta) = meta_path {
                        match remove_file_if_matches(Path::new(meta), None) {
                            Ok(true) => removed += 1,
                            Ok(false) => {
                                kept += 1;
                                notes.push(format!(
                                    "保留任务 {}（元数据已不存在）",
                                    record.id.clone().unwrap_or_default()
                                ));
                            }
                            Err(e) => return Err(e),
                        }
                    }
                    if let Some(session) = session_path {
                        match remove_dir_if_matches(Path::new(session), None) {
                            Ok(true) => removed += 1,
                            Ok(false) => {
                                kept += 1;
                                notes.push(format!(
                                    "保留会话目录 {}（已不存在）",
                                    record.id.clone().unwrap_or_default()
                                ));
                            }
                            Err(e) => return Err(e),
                        }
                    }
                } else {
                    kept += 1;
                    notes.push(format!(
                        "保留任务 {}（迁移后被修改，元数据与会话目录一并保留）",
                        record.id.clone().unwrap_or_default()
                    ));
                }
            }
            COMPONENT_PROJECTS => {
                if let Some(target) = &record.target {
                    match remove_dir_if_matches(Path::new(target), record.fingerprint.as_ref()) {
                        Ok(true) => removed += 1,
                        Ok(false) => {
                            kept += 1;
                            notes.push(format!(
                                "保留 Project 缓存 {}（已不存在或已被修改）",
                                record.id.clone().unwrap_or_default()
                            ));
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            COMPONENT_SCHEDULES => {
                if let (Some(target), Some(ids)) = (&record.target, &record.ids) {
                    match revert_registry_removal(target, ids, /*is_object=*/ true) {
                        Ok(n) => reverted += n,
                        Err(e) => return Err(e),
                    }
                }
            }
            COMPONENT_ARTIFACTS => {
                if let (Some(target), Some(ids)) = (&record.target, &record.ids) {
                    match revert_registry_removal(target, ids, /*is_object=*/ false) {
                        Ok(n) => reverted += n,
                        Err(e) => return Err(e),
                    }
                }
            }
            _ => {}
        }
    }

    Ok(MigrationRestoreResult {
        backup_path,
        removed_count: removed,
        reverted_count: reverted,
        kept_count: kept,
        skipped_reason: None,
        notes: if notes.is_empty() { None } else { Some(notes) },
    })
}

/// Remove only the given IDs from a registry file (scheduled-tasks.json object
/// or artifacts.json array), preserving any entries added after the migration.
/// Returns the number of IDs actually removed.
fn revert_registry_removal(
    target: &str,
    ids: &[String],
    is_object: bool,
) -> Result<usize, AppError> {
    let path = Path::new(target);
    if !path.is_file() {
        return Ok(0);
    }
    let id_set: HashSet<&str> = ids.iter().map(|s| s.as_str()).collect();
    let mut data = read_json_value(path)?;
    let mut removed = 0;

    if is_object {
        let Some(obj) = data.as_object_mut() else {
            return Ok(0);
        };
        if let Some(Value::Array(tasks)) = obj.get_mut("scheduledTasks") {
            tasks.retain(|t| {
                // Remove only IDs this migration installed; entries without an
                // id (e.g. in-progress drafts) must be preserved.
                let keep = match t.get("id").and_then(Value::as_str) {
                    Some(id) => !id_set.contains(id),
                    None => true,
                };
                if !keep {
                    removed += 1;
                }
                keep
            });
        }
    } else if let Some(arr) = data.as_array_mut() {
        arr.retain(|item| {
            let keep = match item.get("id").and_then(Value::as_str) {
                Some(id) => !id_set.contains(id),
                None => true,
            };
            if !keep {
                removed += 1;
            }
            keep
        });
    }

    if removed > 0 {
        let bytes = json_bytes(&data)?;
        crate::config::atomic_write(path, &bytes)?;
    }
    Ok(removed)
}

// ---------------------------------------------------------------------------
// Tests (synthetic fixtures only; no real account IDs, transcripts, or data)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::TempDir;

    // -- fixture helpers ----------------------------------------------------

    fn write_json(path: &Path, value: &Value) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
    }

    fn write_text(path: &Path, text: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, text).unwrap();
    }

    fn sha256_hex(path: &Path) -> String {
        sha256_file(path).unwrap()
    }

    /// Source uses device/account `dev-src`/`acct-src`; target uses
    /// `dev-dst`/`acct-dst`. Roots are built under a per-test temp dir.
    struct Fixture {
        _tmp: TempDir,
        home: PathBuf,
        source_app: PathBuf,
        target_app: PathBuf,
        backup: PathBuf,
    }

    impl Fixture {
        fn new() -> Self {
            let tmp = TempDir::new().unwrap();
            let base = tmp.path();
            let home = base.join("home");
            let source_app = base.join("source");
            let target_app = base.join("target");
            let backup = base.join("backup");
            fs::create_dir_all(&home).unwrap();
            fs::create_dir_all(&source_app).unwrap();
            fs::create_dir_all(&target_app).unwrap();
            fs::create_dir_all(&backup).unwrap();
            Fixture {
                _tmp: tmp,
                home,
                source_app,
                target_app,
                backup,
            }
        }

        fn source_code_root(&self) -> PathBuf {
            self.source_app
                .join(CODE_CONTAINER)
                .join("dev-src")
                .join("acct-src")
        }
        fn target_code_root(&self) -> PathBuf {
            self.target_app
                .join(CODE_CONTAINER)
                .join("dev-dst")
                .join("acct-dst")
        }
        fn source_cowork_root(&self) -> PathBuf {
            self.source_app
                .join(COWORK_CONTAINER)
                .join("dev-src")
                .join("acct-src")
        }
        fn target_cowork_root(&self) -> PathBuf {
            self.target_app
                .join(COWORK_CONTAINER)
                .join("dev-dst")
                .join("acct-dst")
        }

        /// Add a Code index file under the given root.
        fn add_code_index(&self, root: &Path, id: &str, cli_id: &str) {
            write_json(
                &root.join(format!("local_{id}.json")),
                &json!({
                    "sessionId": id,
                    "cliSessionId": cli_id,
                    "cwd": "/work/project",
                    "title": format!("code {id}"),
                }),
            );
        }

        /// Add a shared Claude Code transcript under `home/.claude/projects`.
        fn add_shared_transcript(&self, cli_id: &str) {
            write_text(
                &self
                    .home
                    .join(".claude/projects/-work-project")
                    .join(format!("{cli_id}.jsonl")),
                "{\"role\":\"user\"}\n",
            );
        }

        /// Add a Cowork metadata + same-name session directory.
        fn add_cowork_session(&self, root: &Path, id: &str, meta: &Value, with_dir: bool) {
            write_json(&root.join(format!("local_{id}.json")), meta);
            if with_dir {
                let dir = root.join(format!("local_{id}"));
                write_text(&dir.join("audit.jsonl"), "{\"ev\":1}\n");
                write_text(&dir.join("uploads/in.txt"), "upload-bytes");
                write_text(&dir.join("outputs/out.txt"), "output-bytes");
            }
        }

        fn cowork_meta(id: &str, cwd: &str, folders: &[&str]) -> Value {
            json!({
                "sessionId": id,
                "cliSessionId": format!("cli-{id}"),
                "title": format!("task {id}"),
                "cwd": cwd,
                "originCwd": cwd,
                "userSelectedFolders": folders,
                "fsDetectedFiles": [{"hostPath": format!("{cwd}/notes.txt"), "name": "notes.txt"}],
            })
        }

        /// Seed the target roots so each container resolves to exactly one
        /// account root.
        fn seed_target(&self) {
            self.add_code_index(&self.target_code_root(), "seedcode", "cli-seedcode");
            let seed = self.target_cowork_root();
            self.add_cowork_session(
                &seed,
                "seedcowork",
                &json!({"sessionId":"seedcowork"}),
                true,
            );
        }

        fn no_overrides(&self) -> RootOverrides {
            RootOverrides::default()
        }

        fn apply(&self, components: &[String]) -> Result<MigrationApplyResult, AppError> {
            apply_migration(
                &self.home,
                &self.source_app,
                &self.target_app,
                &self.no_overrides(),
                &[],
                components,
                &self.backup,
                false,
            )
        }

        fn plan(&self) -> Result<MigrationPlan, AppError> {
            build_plan(
                &self.home,
                &self.source_app,
                &self.target_app,
                &self.no_overrides(),
                &[],
            )
        }
    }

    fn all_components() -> Vec<String> {
        DEFAULT_COMPONENTS.iter().map(|s| s.to_string()).collect()
    }

    // -- discovery / resolution ---------------------------------------------

    #[test]
    fn resolves_unique_account_root() {
        let fx = Fixture::new();
        fx.add_code_index(&fx.source_code_root(), "a", "cli-a");
        fx.seed_target();
        let plan = fx.plan().unwrap();
        assert_eq!(plan.code.status, "ready");
        assert_eq!(
            plan.code.source_root.as_deref(),
            Some(fx.source_code_root().to_str().unwrap())
        );
        assert!(plan.blocking_issues.is_empty());
    }

    #[test]
    fn refuses_to_guess_when_multiple_source_roots() {
        let fx = Fixture::new();
        // Two distinct account roots under the source code container.
        fx.add_code_index(&fx.source_code_root(), "a", "cli-a");
        let other = fx
            .source_app
            .join(CODE_CONTAINER)
            .join("dev-src2")
            .join("acct-src2");
        fx.add_code_index(&other, "b", "cli-b");
        fx.seed_target();

        let plan = fx.plan().unwrap();
        assert_eq!(plan.code.status, "ambiguous-source");
        assert_eq!(plan.code.candidates.len(), 2);
        assert!(plan
            .blocking_issues
            .iter()
            .any(|i| i.contains("多个候选账户根目录")));
        // Apply must refuse rather than guess.
        assert!(fx.apply(&all_components()).is_err());

        // An explicit override resolves the ambiguity.
        let overrides = RootOverrides {
            source_code: Some(fx.source_code_root()),
            ..Default::default()
        };
        let plan2 = build_plan(&fx.home, &fx.source_app, &fx.target_app, &overrides, &[]).unwrap();
        assert_eq!(plan2.code.status, "ready");
    }

    #[test]
    fn reports_missing_target_seed() {
        let fx = Fixture::new();
        fx.add_code_index(&fx.source_code_root(), "a", "cli-a");
        // Target has no account root at all (no seed session).
        let plan = fx.plan().unwrap();
        assert_eq!(plan.code.status, "missing-target-seed");
        assert!(plan.blocking_issues.iter().any(|i| i.contains("种子会话")));
        assert!(fx.apply(&all_components()).is_err());
    }

    // -- Code index + shared transcript -------------------------------------

    #[test]
    fn code_index_with_and_without_shared_transcript() {
        let fx = Fixture::new();
        fx.add_code_index(&fx.source_code_root(), "has", "cli-has");
        fx.add_code_index(&fx.source_code_root(), "missing", "cli-missing");
        fx.add_shared_transcript("cli-has");
        fx.seed_target();
        let plan = fx.plan().unwrap();
        // Only the index whose cliSessionId has no shared transcript counts.
        assert_eq!(plan.code.missing_shared_transcripts, 1);
        assert_eq!(plan.code.new_records, 2);
    }

    #[test]
    fn migrates_code_index_without_duplicating_transcript() {
        let fx = Fixture::new();
        fx.add_code_index(&fx.source_code_root(), "a", "cli-a");
        fx.add_shared_transcript("cli-a");
        fx.seed_target();
        let result = fx.apply(&["code".to_string()]).unwrap();
        assert_eq!(result.installed_count, 1);
        let target_meta = fx.target_code_root().join("local_a.json");
        assert!(target_meta.exists());
        // Index copied byte-for-byte (no transform for Code).
        assert_eq!(
            sha256_hex(&fx.source_code_root().join("local_a.json")),
            sha256_hex(&target_meta)
        );
        // Transcript is NOT copied into the target (shared store untouched).
        assert!(!fx.target_app.join(".claude").exists());
    }

    // -- Cowork sessions ------------------------------------------------------

    #[test]
    fn migrates_normal_cowork_session_pair() {
        let fx = Fixture::new();
        let meta = Fixture::cowork_meta("c1", "/work/proj", &[]);
        fx.add_cowork_session(&fx.source_cowork_root(), "c1", &meta, true);
        fx.seed_target();
        let result = fx.apply(&["cowork".to_string()]).unwrap();
        assert_eq!(result.installed_count, 1);
        assert!(fx.target_cowork_root().join("local_c1.json").exists());
        assert!(fx.target_cowork_root().join("local_c1").is_dir());
    }

    #[test]
    fn cowork_payloads_stay_byte_identical() {
        let fx = Fixture::new();
        let meta = Fixture::cowork_meta("c1", "/work/proj", &[]);
        fx.add_cowork_session(&fx.source_cowork_root(), "c1", &meta, true);
        fx.seed_target();
        fx.apply(&["cowork".to_string()]).unwrap();
        let src = fx.source_cowork_root().join("local_c1");
        let dst = fx.target_cowork_root().join("local_c1");
        for rel in ["audit.jsonl", "uploads/in.txt", "outputs/out.txt"] {
            assert_eq!(
                sha256_hex(&src.join(rel)),
                sha256_hex(&dst.join(rel)),
                "{rel}"
            );
        }
        assert_eq!(content_hash_multiset(&src), content_hash_multiset(&dst));
    }

    #[test]
    fn detects_scheduled_cowork_session() {
        let fx = Fixture::new();
        let mut meta = Fixture::cowork_meta("sched", "/work/proj", &[]);
        meta["scheduledTaskId"] = json!("task-123");
        meta["sessionType"] = json!("scheduled");
        fx.add_cowork_session(&fx.source_cowork_root(), "sched", &meta, true);
        fx.seed_target();
        let plan = fx.plan().unwrap();
        assert_eq!(plan.cowork.scheduled_records, 1);
    }

    #[test]
    fn cowork_metadata_missing_dir_is_failed_not_repaired() {
        let fx = Fixture::new();
        // One normal pair, one metadata with no session directory.
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "ok",
            &Fixture::cowork_meta("ok", "/work/p", &[]),
            true,
        );
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "broken",
            &Fixture::cowork_meta("broken", "/work/p", &[]),
            false,
        );
        fx.seed_target();
        let plan = fx.plan().unwrap();
        assert_eq!(plan.cowork.missing_session_directories, 1);

        let result = fx.apply(&["cowork".to_string()]).unwrap();
        assert_eq!(result.installed_count, 1);
        assert_eq!(result.failed_count, 1);
        assert!(result.failed[0]
            .error
            .as_deref()
            .unwrap()
            .contains("会话目录缺失"));
        // The broken one was not installed.
        assert!(!fx.target_cowork_root().join("local_broken.json").exists());
    }

    #[test]
    fn skips_existing_target_id_without_overwrite() {
        let fx = Fixture::new();
        let meta = Fixture::cowork_meta("dup", "/work/proj", &[]);
        fx.add_cowork_session(&fx.source_cowork_root(), "dup", &meta, true);
        fx.seed_target();
        // Pre-existing target session with the same id but different content.
        let existing = json!({"sessionId":"dup","title":"original target copy"});
        fx.add_cowork_session(&fx.target_cowork_root(), "dup", &existing, true);
        let before = sha256_hex(&fx.target_cowork_root().join("local_dup.json"));

        let result = fx.apply(&["cowork".to_string()]).unwrap();
        assert_eq!(result.installed_count, 0);
        assert_eq!(result.skipped_count, 1);
        // Target content untouched.
        assert_eq!(
            before,
            sha256_hex(&fx.target_cowork_root().join("local_dup.json"))
        );
        let data: Value = read_json_file(&fx.target_cowork_root().join("local_dup.json")).unwrap();
        assert_eq!(data["title"], json!("original target copy"));
    }

    #[test]
    fn double_run_is_idempotent() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.add_code_index(&fx.source_code_root(), "k1", "cli-k1");
        fx.seed_target();

        let first = fx.apply(&all_components()).unwrap();
        assert!(first.installed_count >= 2);
        let second = fx.apply(&all_components()).unwrap();
        assert_eq!(second.installed_count, 0, "second run must install nothing");
        // No duplicate sessions exist (id namespace includes the `local_` prefix).
        let count = direct_meta_files(&fx.target_cowork_root())
            .iter()
            .filter(|p| metadata_id(p).as_deref() == Some("local_c1"))
            .count();
        assert_eq!(count, 1);
    }

    // -- Folder binding repair ------------------------------------------------

    #[test]
    fn folder_binding_existing_path_populates_resolved_kinds() {
        let fx = Fixture::new();
        let folder = fx.home.join("real-folder");
        fs::create_dir_all(&folder).unwrap();
        let cwd = folder.to_str().unwrap();
        let meta = Fixture::cowork_meta("c1", cwd, &[cwd]);
        fx.add_cowork_session(&fx.source_cowork_root(), "c1", &meta, true);
        fx.seed_target();
        fx.apply(&["cowork".to_string()]).unwrap();
        let data: Value = read_json_file(&fx.target_cowork_root().join("local_c1.json")).unwrap();
        let kinds = data["resolvedFolderKinds"].as_array().unwrap();
        assert_eq!(kinds.len(), 1);
        assert_eq!(kinds[0]["display"], json!(cwd));
        assert_eq!(kinds[0]["kind"], json!("local"));
    }

    #[test]
    fn relocated_folder_uses_path_map() {
        let fx = Fixture::new();
        let old_folder = fx.home.join("old-location");
        let new_folder = fx.home.join("new-location");
        fs::create_dir_all(&new_folder).unwrap(); // only the NEW path exists
        let old = old_folder.to_str().unwrap().to_string();
        let new = new_folder.to_str().unwrap().to_string();
        let meta = Fixture::cowork_meta("c1", &old, &[&old]);
        fx.add_cowork_session(&fx.source_cowork_root(), "c1", &meta, true);
        fx.seed_target();

        let maps = vec![PathMapping {
            old: old.clone(),
            new: new.clone(),
        }];
        let result = apply_migration(
            &fx.home,
            &fx.source_app,
            &fx.target_app,
            &fx.no_overrides(),
            &maps,
            &["cowork".to_string()],
            &fx.backup,
            false,
        )
        .unwrap();
        assert_eq!(result.installed_count, 1);
        let data: Value = read_json_file(&fx.target_cowork_root().join("local_c1.json")).unwrap();
        assert_eq!(data["cwd"], json!(new));
        assert_eq!(data["userSelectedFolders"], json!([new.clone()]));
        let kinds = data["resolvedFolderKinds"].as_array().unwrap();
        assert_eq!(kinds[0]["display"], json!(new));
        // Structural hostPath under the moved folder is remapped too.
        assert_eq!(
            data["fsDetectedFiles"][0]["hostPath"],
            json!(format!("{new}/notes.txt"))
        );
    }

    #[test]
    fn missing_folder_drops_resolved_kind_but_keeps_reference() {
        let fx = Fixture::new();
        let gone = fx.home.join("no-longer-there");
        let cwd = gone.to_str().unwrap();
        let meta = Fixture::cowork_meta("c1", cwd, &[cwd]);
        fx.add_cowork_session(&fx.source_cowork_root(), "c1", &meta, true);
        fx.seed_target();
        let plan = fx.plan().unwrap();
        assert_eq!(plan.cowork.missing_folder_paths.len(), 1);
        fx.apply(&["cowork".to_string()]).unwrap();
        let data: Value = read_json_file(&fx.target_cowork_root().join("local_c1.json")).unwrap();
        // Reference retained (not fabricated), but no resolved kind created.
        assert_eq!(data["userSelectedFolders"], json!([cwd]));
        assert_eq!(data["resolvedFolderKinds"], json!([]));
    }

    #[test]
    fn does_not_rewrite_prose_that_mentions_a_path() {
        let fx = Fixture::new();
        let old = fx.home.join("old").to_str().unwrap().to_string();
        let new = fx.home.join("new").to_str().unwrap().to_string();
        fs::create_dir_all(fx.home.join("new")).unwrap();
        // A free-text field that merely mentions the old path must survive
        // untouched; only structural path fields are transformed.
        let mut meta = Fixture::cowork_meta("c1", &old, &[]);
        meta["title"] = json!(format!("review notes under {old} please"));
        fx.add_cowork_session(&fx.source_cowork_root(), "c1", &meta, true);
        fx.seed_target();
        let maps = vec![PathMapping {
            old: old.clone(),
            new: new.clone(),
        }];
        apply_migration(
            &fx.home,
            &fx.source_app,
            &fx.target_app,
            &fx.no_overrides(),
            &maps,
            &["cowork".to_string()],
            &fx.backup,
            false,
        )
        .unwrap();
        let data: Value = read_json_file(&fx.target_cowork_root().join("local_c1.json")).unwrap();
        assert_eq!(data["cwd"], json!(new)); // structural field transformed
        assert_eq!(
            data["title"],
            json!(format!("review notes under {old} please")) // prose untouched
        );
    }

    // -- Projects / artifacts / schedules -------------------------------------

    #[test]
    fn copies_missing_project_cache_only() {
        let fx = Fixture::new();
        let cache = fx.source_cowork_root().join(".project-cache");
        write_json(
            &cache.join("proj-1/metadata.json"),
            &json!({"uuid":"proj-1"}),
        );
        write_json(
            &cache.join("proj-2/metadata.json"),
            &json!({"uuid":"proj-2"}),
        );
        // proj-2 already exists in target.
        write_json(
            &fx.target_cowork_root()
                .join(".project-cache/proj-2/metadata.json"),
            &json!({"uuid":"proj-2"}),
        );
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "pb",
            &{
                let mut m = Fixture::cowork_meta("pb", "/work/p", &[]);
                m["userSelectedProjectUuids"] = json!(["proj-1", "proj-2"]);
                m
            },
            true,
        );
        fx.seed_target();
        let result = fx.apply(&["projects".to_string()]).unwrap();
        assert_eq!(result.installed_count, 1); // only proj-1
        assert!(fx
            .target_cowork_root()
            .join(".project-cache/proj-1/metadata.json")
            .exists());
    }

    #[test]
    fn artifact_merge_skips_missing_entity() {
        let fx = Fixture::new();
        write_json(
            &fx.source_cowork_root().join("artifacts.json"),
            &json!([{"id":"art-present"},{"id":"art-missing"}]),
        );
        // Only one entity directory exists in the shared Documents store.
        write_text(
            &fx.home
                .join("Documents/Claude/Artifacts/art-present/index.html"),
            "<html></html>",
        );
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "s",
            &Fixture::cowork_meta("s", "/work/p", &[]),
            true,
        );
        fx.seed_target();
        let result = fx.apply(&["artifacts".to_string()]).unwrap();
        let registry: Value =
            read_json_file(&fx.target_cowork_root().join("artifacts.json")).unwrap();
        let ids: Vec<&str> = registry
            .as_array()
            .unwrap()
            .iter()
            .map(|i| i["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec!["art-present"]);
        // The missing-entity artifact is reported as skipped, not installed.
        assert!(result.failed.is_empty());
    }

    #[test]
    fn imported_scheduled_definitions_stay_disabled() {
        let fx = Fixture::new();
        write_json(
            &fx.source_cowork_root().join("scheduled-tasks.json"),
            &json!({
                "scheduledTasks": [
                    {"id":"task-1","enabled":true,"filePath":"/old/run.md","userSelectedFolders":["/old"]}
                ]
            }),
        );
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "s",
            &Fixture::cowork_meta("s", "/work/p", &[]),
            true,
        );
        fx.seed_target();
        fx.apply(&["schedules".to_string()]).unwrap();
        let data: Value =
            read_json_file(&fx.target_cowork_root().join("scheduled-tasks.json")).unwrap();
        let tasks = data["scheduledTasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0]["id"], json!("task-1"));
        assert_eq!(tasks[0]["enabled"], json!(false), "import must be disabled");
    }

    // -- Atomicity / interruption ---------------------------------------------

    #[test]
    fn leaves_no_staging_artifacts_after_apply() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.seed_target();
        fx.apply(&["cowork".to_string()]).unwrap();
        let leftovers: Vec<_> = fs::read_dir(fx.target_cowork_root())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".migration-"))
            .collect();
        assert!(leftovers.is_empty(), "no staging leftovers: {leftovers:?}");
    }

    #[test]
    fn stale_staging_dir_does_not_block_or_duplicate() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.seed_target();
        // Simulate a previously interrupted run that left a staging dir behind.
        fs::create_dir_all(fx.target_cowork_root().join(".local_c1.migration-stale")).unwrap();
        let result = fx.apply(&["cowork".to_string()]).unwrap();
        assert_eq!(result.installed_count, 1);
        assert!(fx.target_cowork_root().join("local_c1").is_dir());
    }

    // -- Source immutability ----------------------------------------------------

    #[test]
    fn source_files_are_never_modified() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.add_code_index(&fx.source_code_root(), "k1", "cli-k1");
        fx.seed_target();
        let before = content_hash_multiset(&fx.source_app);
        fx.apply(&all_components()).unwrap();
        let after = content_hash_multiset(&fx.source_app);
        assert_eq!(before, after, "source tree must be read-only");
    }

    // -- Restore ----------------------------------------------------------------

    #[test]
    fn restore_removes_only_installed_and_preserves_new_sessions() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.add_code_index(&fx.source_code_root(), "k1", "cli-k1");
        fx.seed_target();
        let apply = fx.apply(&all_components()).unwrap();
        assert!(apply.installed_count >= 2);

        // A session created AFTER the migration must survive restore.
        fx.add_cowork_session(
            &fx.target_cowork_root(),
            "post",
            &json!({"sessionId":"post"}),
            true,
        );

        let ledger_path = latest_ledger_path(&fx.backup).unwrap();
        let restore = restore_migration(Some(&ledger_path), &fx.backup).unwrap();
        assert!(restore.removed_count >= 1);
        // Migrated records removed.
        assert!(!fx.target_cowork_root().join("local_c1.json").exists());
        assert!(!fx.target_cowork_root().join("local_c1").exists());
        assert!(!fx.target_code_root().join("local_k1.json").exists());
        // Seed + post-migration sessions preserved.
        assert!(fx
            .target_cowork_root()
            .join("local_seedcowork.json")
            .exists());
        assert!(fx.target_cowork_root().join("local_post.json").exists());
        assert!(fx.target_code_root().join("local_seedcode.json").exists());
    }

    #[test]
    fn restore_without_ledger_reports_skipped() {
        let fx = Fixture::new();
        let restore = restore_migration(None, &fx.backup).unwrap();
        assert_eq!(restore.skipped_reason.as_deref(), Some("no-ledger"));
        assert_eq!(restore.removed_count, 0);
    }

    // -- Paths with spaces / apostrophes / non-ASCII ----------------------------

    #[test]
    fn handles_spaces_apostrophes_and_non_ascii_paths() {
        let tmp = TempDir::new().unwrap();
        let base = tmp.path().join("weird dir's 中文");
        let home = base.join("home");
        let source_app = base.join("source app");
        let target_app = base.join("target app");
        let backup = base.join("backup");
        for p in [&home, &source_app, &target_app, &backup] {
            fs::create_dir_all(p).unwrap();
        }
        let src_cowork = source_app
            .join(COWORK_CONTAINER)
            .join("dev src")
            .join("acct src");
        let dst_cowork = target_app
            .join(COWORK_CONTAINER)
            .join("dev dst")
            .join("acct dst");
        let meta = json!({
            "sessionId":"c1","cliSessionId":"cli-c1","cwd":"/work/项目 dir",
            "userSelectedFolders": [],
        });
        write_json(&src_cowork.join("local_c1.json"), &meta);
        write_text(&src_cowork.join("local_c1/audit.jsonl"), "审计\n");
        // Seed target cowork.
        write_json(
            &dst_cowork.join("local_seed.json"),
            &json!({"sessionId":"seed"}),
        );
        write_text(&dst_cowork.join("local_seed/audit.jsonl"), "seed\n");

        let result = apply_migration(
            &home,
            &source_app,
            &target_app,
            &RootOverrides::default(),
            &[],
            &["cowork".to_string()],
            &backup,
            false,
        )
        .unwrap();
        assert_eq!(result.installed_count, 1);
        assert_eq!(
            sha256_hex(&src_cowork.join("local_c1/audit.jsonl")),
            sha256_hex(&dst_cowork.join("local_c1/audit.jsonl"))
        );
    }

    // -- Verify -----------------------------------------------------------------

    #[test]
    fn verify_passes_after_clean_migration() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.add_code_index(&fx.source_code_root(), "k1", "cli-k1");
        fx.add_shared_transcript("cli-k1");
        fx.seed_target();
        // The pre-existing 3P seed session is healthy: it has its shared
        // transcript, so a clean migration verifies cleanly.
        fx.add_shared_transcript("cli-seedcode");
        fx.apply(&all_components()).unwrap();
        let verify = verify_migration(
            &fx.home,
            &fx.source_app,
            &fx.target_app,
            &fx.no_overrides(),
            &all_components(),
        )
        .unwrap();
        assert!(verify.passed, "checks: {:?}", verify.checks);
    }

    #[test]
    fn verify_flags_missing_source_ids() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.seed_target();
        // No apply: source id is missing from target.
        let verify = verify_migration(
            &fx.home,
            &fx.source_app,
            &fx.target_app,
            &fx.no_overrides(),
            &all_components(),
        )
        .unwrap();
        assert!(!verify.passed);
        assert!(verify.checks.iter().any(|c| !c.ok));
    }

    // -- Audit ------------------------------------------------------------------

    #[test]
    fn audit_is_read_only_and_counts_candidates() {
        let fx = Fixture::new();
        fx.add_code_index(&fx.source_code_root(), "a", "cli-a");
        fx.add_shared_transcript("cli-a");
        fx.seed_target();
        let before = content_hash_multiset(&fx.source_app);
        let audit = build_audit(&fx.home, &fx.source_app, &fx.target_app);
        assert_eq!(audit.source_app.code_roots.len(), 1);
        assert_eq!(audit.source_app.code_roots[0].metadata_count, 1);
        assert_eq!(audit.shared_code_transcript_count, 1);
        assert!(audit.target_app.exists);
        // Audit never writes.
        assert_eq!(before, content_hash_multiset(&fx.source_app));
    }

    // -- Path map parsing ---------------------------------------------------------

    #[test]
    fn path_maps_must_be_absolute() {
        let bad = vec![PathMapping {
            old: "relative/path".to_string(),
            new: "/abs".to_string(),
        }];
        assert!(parse_path_maps(&bad).is_err());
        let good = vec![
            PathMapping {
                old: "/a/b".to_string(),
                new: "/x/y".to_string(),
            },
            PathMapping {
                old: "/a".to_string(),
                new: "/x".to_string(),
            },
        ];
        let parsed = parse_path_maps(&good).unwrap();
        // Longest prefix first.
        assert_eq!(parsed[0].0, "/a/b");
    }

    #[test]
    fn prefix_replace_respects_boundaries() {
        assert_eq!(prefix_replace("/old", "/old", "/new"), "/new");
        assert_eq!(prefix_replace("/old/sub", "/old", "/new"), "/new/sub");
        // Not a boundary: "/oldish" must NOT match "/old".
        assert_eq!(prefix_replace("/oldish", "/old", "/new"), "/oldish");
    }

    // -- Codex review findings -------------------------------------------------

    #[test]
    fn partial_failure_returns_apply_error_and_persists_ledger() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.add_code_index(&fx.source_code_root(), "k1", "cli-k1");
        fx.seed_target();
        // Source definitions exist, but the target registry is malformed so
        // merge_scheduled_tasks fails AFTER code/cowork already wrote records.
        write_json(
            &fx.source_cowork_root().join("scheduled-tasks.json"),
            &json!({"scheduledTasks": [{"id": "task-1", "name": "one"}]}),
        );
        write_text(
            &fx.target_cowork_root().join("scheduled-tasks.json"),
            "{not-json",
        );
        let result = fx.apply(&all_components()).unwrap();
        assert!(
            result.apply_error.is_some(),
            "later component failure must be reported, not discarded: {result:?}"
        );
        assert!(
            result.installed_count >= 2,
            "code/cowork records must remain installed"
        );
        // The ledger that owns the writes is on disk and drives a clean undo.
        let ledger_path = latest_ledger_path(&fx.backup).unwrap();
        let ledger = load_ledger(&ledger_path).unwrap();
        assert!(ledger
            .installed
            .iter()
            .any(|r| r.component == COMPONENT_CODE));
        assert!(ledger
            .installed
            .iter()
            .any(|r| r.component == COMPONENT_COWORK));
        let restore = restore_migration(Some(&ledger_path), &fx.backup).unwrap();
        assert!(restore.removed_count >= 3);
        assert!(!fx.target_cowork_root().join("local_c1.json").exists());
        assert!(!fx.target_cowork_root().join("local_c1").exists());
        assert!(!fx.target_code_root().join("local_k1.json").exists());
    }

    #[test]
    fn restore_keeps_task_whose_metadata_was_modified_after_migration() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.seed_target();
        let apply = fx.apply(&["cowork".to_string()]).unwrap();
        assert_eq!(apply.installed_count, 1);
        // The user continues the migrated task: metadata gains a new field.
        let meta_path = fx.target_cowork_root().join("local_c1.json");
        let mut data = read_json_value(&meta_path).unwrap();
        data.as_object_mut()
            .unwrap()
            .insert("continuedAt".to_string(), json!("2026-07-31T12:00:00Z"));
        write_json(&meta_path, &data);
        let ledger_path = latest_ledger_path(&fx.backup).unwrap();
        let restore = restore_migration(Some(&ledger_path), &fx.backup).unwrap();
        assert_eq!(restore.removed_count, 0);
        assert!(restore.kept_count >= 1);
        assert!(meta_path.exists(), "modified metadata must be preserved");
        assert!(
            fx.target_cowork_root().join("local_c1").is_dir(),
            "paired session dir must be preserved (never orphan the task)"
        );
    }

    #[test]
    fn restore_keeps_session_dir_with_same_size_content_edit() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.seed_target();
        fx.apply(&["cowork".to_string()]).unwrap();
        // Same-size edit: same byte length, different content.
        let audit = fx.target_cowork_root().join("local_c1/audit.jsonl");
        fs::write(&audit, "{\"ev\":9}\n").unwrap();
        let ledger_path = latest_ledger_path(&fx.backup).unwrap();
        let restore = restore_migration(Some(&ledger_path), &fx.backup).unwrap();
        assert_eq!(restore.removed_count, 0);
        assert!(fx.target_cowork_root().join("local_c1").is_dir());
    }

    #[test]
    fn restore_keeps_project_cache_with_same_size_edit() {
        let fx = Fixture::new();
        // The Cowork account root must be discoverable (needs a local_* meta).
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "seed",
            &json!({"sessionId": "seed"}),
            true,
        );
        let src_cache = fx.source_cowork_root().join(".project-cache");
        fs::create_dir_all(src_cache.join("proj-a")).unwrap();
        write_text(&src_cache.join("proj-a/index.json"), "{\"k\":1}");
        fx.seed_target();
        let apply = fx.apply(&["projects".to_string()]).unwrap();
        assert_eq!(apply.installed_count, 1);
        // Same-size content edit after migration.
        let index = fx
            .target_cowork_root()
            .join(".project-cache/proj-a/index.json");
        fs::write(&index, "{\"k\":2}").unwrap();
        let ledger_path = latest_ledger_path(&fx.backup).unwrap();
        let restore = restore_migration(Some(&ledger_path), &fx.backup).unwrap();
        assert_eq!(restore.removed_count, 0);
        assert!(fx
            .target_cowork_root()
            .join(".project-cache/proj-a")
            .is_dir());
    }

    #[test]
    fn restore_preserves_registry_entries_lacking_an_id() {
        let fx = Fixture::new();
        // Make the Cowork account root discoverable.
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "seed",
            &json!({"sessionId": "seed"}),
            true,
        );
        write_json(
            &fx.source_cowork_root().join("scheduled-tasks.json"),
            &json!({"scheduledTasks": [{"id": "task-src-1", "name": "one"}]}),
        );
        write_json(
            &fx.source_cowork_root().join("artifacts.json"),
            &json!([{"id": "art-src-1", "name": "a"}]),
        );
        // Artifact entities live under the shared Documents root; create the
        // entity so the registry entry is actually installed.
        fs::create_dir_all(fx.home.join("Documents/Claude/Artifacts/art-src-1")).unwrap();
        fx.seed_target();
        let apply = fx
            .apply(&["schedules".to_string(), "artifacts".to_string()])
            .unwrap();
        assert_eq!(apply.installed_count, 2);
        // In-progress entries created after the migration, without an id.
        let sched_path = fx.target_cowork_root().join("scheduled-tasks.json");
        let mut sched = read_json_value(&sched_path).unwrap();
        sched["scheduledTasks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "draft-no-id"}));
        write_json(&sched_path, &sched);
        let art_path = fx.target_cowork_root().join("artifacts.json");
        let mut art = read_json_value(&art_path).unwrap();
        art.as_array_mut()
            .unwrap()
            .push(json!({"name": "draft-artifact"}));
        write_json(&art_path, &art);

        let ledger_path = latest_ledger_path(&fx.backup).unwrap();
        let restore = restore_migration(Some(&ledger_path), &fx.backup).unwrap();
        assert!(restore.reverted_count >= 2);
        // Installed ids removed; id-less entries survive.
        let sched_after = read_json_value(&sched_path).unwrap();
        let tasks = sched_after["scheduledTasks"].as_array().unwrap();
        assert_eq!(tasks.len(), 1, "draft without id must survive undo");
        assert!(tasks[0].get("id").is_none());
        let art_after = read_json_value(&art_path).unwrap();
        assert_eq!(art_after.as_array().unwrap().len(), 1);
        assert!(art_after[0].get("id").is_none());
    }

    #[test]
    fn verify_honors_selected_components() {
        let fx = Fixture::new();
        fx.add_cowork_session(
            &fx.source_cowork_root(),
            "c1",
            &Fixture::cowork_meta("c1", "/work/p", &[]),
            true,
        );
        fx.add_code_index(&fx.source_code_root(), "k1", "cli-k1");
        fx.seed_target();
        // Only Cowork is applied; Code is intentionally left unmigrated.
        fx.apply(&["cowork".to_string()]).unwrap();
        let partial = verify_migration(
            &fx.home,
            &fx.source_app,
            &fx.target_app,
            &fx.no_overrides(),
            &["cowork".to_string()],
        )
        .unwrap();
        assert!(
            partial.passed,
            "verification scoped to applied components must pass: {:?}",
            partial.checks
        );
        // A full verification still flags the intentionally-unmigrated code ids.
        let full = verify_migration(
            &fx.home,
            &fx.source_app,
            &fx.target_app,
            &fx.no_overrides(),
            &all_components(),
        )
        .unwrap();
        assert!(!full.passed);
    }
}
