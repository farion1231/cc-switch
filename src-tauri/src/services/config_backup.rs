//! Per-app local config backup and rollback service.
//!
//! Captures byte-exact snapshots of each compatible application's local config
//! files on initial import and before edits/saves, so users can inspect and roll
//! back to any saved snapshot at any time.
//!
//! Security boundary:
//! - Backup files live under `<device>/.cc-switch/backups/app-configs/<app>/<id>/`.
//! - Restore never writes to arbitrary paths read from `manifest.json`: every file
//!   key is resolved back to a safe target path via `resolve_target_path_for_key`.
//! - Archive relative paths are strictly validated against traversal.

use std::collections::HashMap;
use std::fs;
use std::io::ErrorKind;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::app_config::AppType;
use crate::config::{
    atomic_write, atomic_write_private, get_claude_config_dir, get_claude_settings_path,
};
use crate::error::AppError;
use crate::live::engine::{lock_app, DeviceStore};
use crate::settings::effective_backup_retain_count;
use crate::store::AppState;
use crate::sync_utils::MutexExt;

const MANIFEST_FILENAME: &str = "manifest.json";
const FILES_DIRNAME: &str = "files";
const MAX_PREVIEW_BYTES: usize = 64 * 1024;

static SEQ_COUNTER: AtomicU64 = AtomicU64::new(0);

fn backup_ops_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Tracks the most recently rolled-back content digest per `(backup_dir, app)` so
/// immediately saving after a rollback does not create a redundant snapshot.
fn last_restored_digests() -> &'static Mutex<HashMap<(PathBuf, String), String>> {
    static MAP: OnceLock<Mutex<HashMap<(PathBuf, String), String>>> = OnceLock::new();
    MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

fn floor_utf8_boundary(bytes: &[u8], limit: usize) -> usize {
    let mut end = limit.min(bytes.len());
    while end > 0 && std::str::from_utf8(&bytes[..end]).is_err() && end > limit.saturating_sub(4) {
        end -= 1;
    }
    end
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ConfigBackupReason {
    Import,
    Edit,
    Rollback,
    Manual,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupFileRecord {
    pub key: String,
    pub archive_rel_path: Option<String>,
    pub original_path: String,
    pub existed: bool,
    pub size_bytes: u64,
    pub sha256: Option<String>,
    pub private: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupManifest {
    pub id: String,
    pub app: String,
    pub reason: ConfigBackupReason,
    pub created_at: String,
    pub pinned: bool,
    pub label: Option<String>,
    pub content_digest: String,
    pub files: Vec<ConfigBackupFileRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupFileSummary {
    pub key: String,
    pub original_path: String,
    pub existed: bool,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupEntry {
    pub id: String,
    pub app: String,
    pub reason: ConfigBackupReason,
    pub created_at: String,
    pub pinned: bool,
    pub label: Option<String>,
    pub total_size_bytes: u64,
    pub files: Vec<ConfigBackupFileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupFilePreview {
    pub key: String,
    pub original_path: String,
    pub existed: bool,
    pub size_bytes: u64,
    pub content: Option<String>,
    pub truncated: bool,
    pub binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupPreview {
    pub entry: ConfigBackupEntry,
    pub files: Vec<ConfigBackupFilePreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigBackupRestoreResult {
    pub app: String,
    pub restored_backup_id: String,
    pub safety_backup_id: Option<String>,
    pub restored_files: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct ConfigTarget {
    pub key: String,
    pub path: PathBuf,
    pub private: bool,
}

impl ConfigTarget {
    fn new(key: impl Into<String>, path: PathBuf, private: bool) -> Self {
        Self {
            key: key.into(),
            path,
            private,
        }
    }
}

/// In unit tests that do not isolate `CC_SWITCH_TEST_HOME` (for example tests
/// creating raw `tempdir` files directly), skip implicit global backups so tests
/// never mutate the developer's real `~/.cc-switch/backups/app-configs`.
pub(crate) fn is_auto_backup_enabled() -> bool {
    #[cfg(test)]
    {
        std::env::var_os("CC_SWITCH_TEST_HOME")
            .is_some_and(|v| !v.to_string_lossy().trim().is_empty())
    }
    #[cfg(not(test))]
    {
        true
    }
}

pub(crate) fn is_device_store(store: &DeviceStore) -> bool {
    store.app_config_backup_dir() == DeviceStore::for_device().app_config_backup_dir()
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Resolve all local config file targets for a given application.
///
/// Exhaustive match over `AppType` ensures every supported app is handled.
pub(crate) fn resolve_app_config_targets(app: &AppType) -> Result<Vec<ConfigTarget>, AppError> {
    match app {
        AppType::Claude => {
            let path = get_claude_settings_path();
            let key = path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| *n == "claude.json" || *n == "settings.json")
                .unwrap_or("settings.json")
                .to_string();
            Ok(vec![ConfigTarget::new(key, path, true)])
        }
        AppType::ClaudeDesktop => {
            let targets = crate::claude_desktop_config::backup_target_paths()?;
            Ok(targets
                .into_iter()
                .map(|(key, path, private)| ConfigTarget::new(key, path, private))
                .collect())
        }
        AppType::Codex => Ok(vec![
            ConfigTarget::new(
                "auth.json",
                crate::codex_config::get_codex_auth_path(),
                true,
            ),
            ConfigTarget::new(
                "config.toml",
                crate::codex_config::get_codex_config_path(),
                true,
            ),
            ConfigTarget::new(
                "cc-switch-model-catalog.json",
                crate::codex_config::get_codex_model_catalog_path(),
                false,
            ),
        ]),
        AppType::Gemini => Ok(vec![
            ConfigTarget::new(".env", crate::gemini_config::get_gemini_env_path(), true),
            ConfigTarget::new(
                "settings.json",
                crate::gemini_config::get_gemini_settings_path(),
                false,
            ),
        ]),
        AppType::GrokBuild => Ok(vec![ConfigTarget::new(
            "config.toml",
            crate::grok_config::get_grok_config_path(),
            true,
        )]),
        AppType::OpenCode => {
            let main_path = crate::opencode_config::get_opencode_config_path()?;
            let main_key = main_path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| *n == "opencode.jsonc" || *n == "opencode.json")
                .unwrap_or("opencode.json")
                .to_string();
            let mut targets = vec![ConfigTarget::new(main_key, main_path, false)];
            for (key, path, private) in crate::services::OmoService::backup_target_paths()? {
                targets.push(ConfigTarget::new(key, path, private));
            }
            Ok(targets)
        }
        AppType::OpenClaw => Ok(vec![ConfigTarget::new(
            "openclaw.json",
            crate::openclaw_config::get_openclaw_config_path(),
            true,
        )]),
        AppType::Hermes => Ok(vec![ConfigTarget::new(
            "config.yaml",
            crate::hermes_config::get_hermes_config_path(),
            true,
        )]),
        AppType::Pi => Ok(vec![
            ConfigTarget::new("models.json", crate::pi_config::get_pi_models_path()?, true),
            ConfigTarget::new(
                "settings.json",
                crate::pi_config::get_pi_settings_path()?,
                false,
            ),
        ]),
        AppType::Mcode => Ok(vec![ConfigTarget::new(
            "config.yaml",
            crate::mcode_config::config_path(),
            true,
        )]),
        AppType::Kimi => Ok(vec![ConfigTarget::new(
            "config.toml",
            crate::kimi_config::config_path(),
            true,
        )]),
        AppType::Dsh => Ok(crate::dsh_config::backup_target_paths()
            .into_iter()
            .map(|(key, path, private)| ConfigTarget::new(key, path, private))
            .collect()),
        AppType::Agy => Ok(vec![ConfigTarget::new(
            "settings.json",
            crate::agy_config::settings_path(),
            true,
        )]),
        AppType::Omp => {
            let models_path = crate::omp_config::get_omp_models_path()?;
            let models_key = models_path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| *n == "models.yml" || *n == "models.yaml")
                .unwrap_or("models.yml")
                .to_string();
            let settings_path = crate::omp_config::get_omp_settings_path()?;
            let settings_key = settings_path
                .file_name()
                .and_then(|n| n.to_str())
                .filter(|n| *n == "config.yml" || *n == "config.yaml")
                .unwrap_or("config.yml")
                .to_string();
            Ok(vec![
                ConfigTarget::new(models_key, models_path, true),
                ConfigTarget::new(settings_key, settings_path, false),
            ])
        }
        AppType::Copilot => Ok(vec![
            ConfigTarget::new(
                "settings.json",
                crate::copilot_config::settings_path(),
                false,
            ),
            ConfigTarget::new(
                "providers.json",
                crate::copilot_config::providers_path(),
                true,
            ),
        ]),
        AppType::WorkBuddy => Ok(vec![ConfigTarget::new(
            "models.json",
            crate::workbuddy_config::config_path(),
            true,
        )]),
    }
}

/// Safely resolve a recorded file key back to the destination path on disk.
/// Never accepts arbitrary filesystem paths from `manifest.json`.
fn resolve_target_path_for_key(
    app: &AppType,
    key: &str,
) -> Result<Option<(PathBuf, bool)>, AppError> {
    if !is_safe_rel_key(key) {
        return Ok(None);
    }

    match app {
        AppType::Claude => match key {
            "settings.json" | "claude.json" => Ok(Some((get_claude_config_dir().join(key), true))),
            _ => Ok(None),
        },
        AppType::OpenCode => {
            if key == "opencode.json" || key == "opencode.jsonc" {
                return Ok(Some((
                    crate::opencode_config::get_opencode_dir().join(key),
                    false,
                )));
            }
            Ok(crate::services::OmoService::resolve_backup_target_path(key))
        }
        AppType::Dsh => Ok(crate::dsh_config::resolve_backup_target_path(key)),
        AppType::Omp => {
            let dir = crate::omp_config::get_omp_agent_dir()?;
            match key {
                "models.yml" | "models.yaml" => Ok(Some((dir.join(key), true))),
                "config.yml" | "config.yaml" => Ok(Some((dir.join(key), false))),
                _ => Ok(None),
            }
        }
        _ => {
            let targets = resolve_app_config_targets(app)?;
            Ok(targets
                .into_iter()
                .find(|t| t.key == key)
                .map(|t| (t.path, t.private)))
        }
    }
}

fn is_safe_rel_key(key: &str) -> bool {
    if key.is_empty() || key.contains('\\') {
        return false;
    }
    let path = Path::new(key);
    !path.is_absolute() && path.components().all(|c| matches!(c, Component::Normal(_)))
}

fn validate_backup_id(backup_id: &str) -> Result<(), AppError> {
    if backup_id.is_empty()
        || backup_id.contains('/')
        || backup_id.contains('\\')
        || backup_id.contains("..")
    {
        return Err(AppError::InvalidInput(format!(
            "非法的备份 ID: {backup_id}"
        )));
    }
    Ok(())
}

fn app_backup_dir(store: &DeviceStore, app: &AppType) -> PathBuf {
    store.app_config_backup_dir().join(app.as_str())
}

fn next_backup_id(app_dir: &Path, reason: ConfigBackupReason) -> String {
    let ts = Utc::now().format("%Y%m%d_%H%M%S_%3f");
    let tag = match reason {
        ConfigBackupReason::Import => "import",
        ConfigBackupReason::Edit => "edit",
        ConfigBackupReason::Rollback => "rollback",
        ConfigBackupReason::Manual => "manual",
    };
    let seq = SEQ_COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut candidate = format!("{ts}_{seq}_{tag}");
    let mut extra = 1u32;
    while app_dir.join(&candidate).exists() {
        candidate = format!("{ts}_{seq}_{extra}_{tag}");
        extra += 1;
    }
    candidate
}

struct CapturedSnapshot {
    records: Vec<ConfigBackupFileRecord>,
    bytes_by_key: HashMap<String, Vec<u8>>,
    content_digest: String,
    any_existed: bool,
}

fn capture_targets(targets: &[ConfigTarget]) -> Result<CapturedSnapshot, AppError> {
    let mut records = Vec::with_capacity(targets.len());
    let mut bytes_by_key = HashMap::new();
    let mut any_existed = false;

    for target in targets {
        match fs::read(&target.path) {
            Ok(bytes) => {
                any_existed = true;
                let size_bytes = bytes.len() as u64;
                let hash = sha256_hex(&bytes);
                let archive_rel_path = format!("{FILES_DIRNAME}/{}", target.key);
                records.push(ConfigBackupFileRecord {
                    key: target.key.clone(),
                    archive_rel_path: Some(archive_rel_path),
                    original_path: target.path.to_string_lossy().into_owned(),
                    existed: true,
                    size_bytes,
                    sha256: Some(hash),
                    private: target.private,
                });
                bytes_by_key.insert(target.key.clone(), bytes);
            }
            Err(err) if err.kind() == ErrorKind::NotFound => {
                records.push(ConfigBackupFileRecord {
                    key: target.key.clone(),
                    archive_rel_path: None,
                    original_path: target.path.to_string_lossy().into_owned(),
                    existed: false,
                    size_bytes: 0,
                    sha256: None,
                    private: target.private,
                });
            }
            Err(err) => return Err(AppError::io(&target.path, err)),
        }
    }

    let mut digest_parts: Vec<String> = records
        .iter()
        .map(|r| {
            format!(
                "{}:{}:{}",
                r.key,
                r.existed,
                r.sha256.as_deref().unwrap_or("")
            )
        })
        .collect();
    digest_parts.sort();
    let content_digest = sha256_hex(digest_parts.join("\n").as_bytes());

    Ok(CapturedSnapshot {
        records,
        bytes_by_key,
        content_digest,
        any_existed,
    })
}

fn read_manifest(snapshot_dir: &Path) -> Result<ConfigBackupManifest, AppError> {
    let manifest_path = snapshot_dir.join(MANIFEST_FILENAME);
    let raw = fs::read_to_string(&manifest_path).map_err(|e| AppError::io(&manifest_path, e))?;
    serde_json::from_str::<ConfigBackupManifest>(&raw)
        .map_err(|e| AppError::json(&manifest_path, e))
}

fn manifest_to_entry(manifest: &ConfigBackupManifest) -> ConfigBackupEntry {
    let total_size_bytes = manifest.files.iter().map(|f| f.size_bytes).sum();
    let files = manifest
        .files
        .iter()
        .map(|f| ConfigBackupFileSummary {
            key: f.key.clone(),
            original_path: f.original_path.clone(),
            existed: f.existed,
            size_bytes: f.size_bytes,
        })
        .collect();
    ConfigBackupEntry {
        id: manifest.id.clone(),
        app: manifest.app.clone(),
        reason: manifest.reason,
        created_at: manifest.created_at.clone(),
        pinned: manifest.pinned,
        label: manifest.label.clone(),
        total_size_bytes,
        files,
    }
}

fn load_app_manifests(store: &DeviceStore, app: &AppType) -> Vec<ConfigBackupManifest> {
    let app_dir = app_backup_dir(store, app);
    let Ok(entries) = fs::read_dir(&app_dir) else {
        return Vec::new();
    };
    let mut manifests = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        match read_manifest(&path) {
            Ok(manifest) if manifest.app == app.as_str() => manifests.push(manifest),
            Ok(manifest) => {
                log::warn!(
                    "Ignoring mismatched app config backup manifest {} (app={})",
                    path.display(),
                    manifest.app
                );
            }
            Err(err) => {
                log::warn!(
                    "Skipping unreadable app config backup {}: {err}",
                    path.display()
                );
            }
        }
    }
    manifests.sort_by(|a, b| {
        b.created_at
            .cmp(&a.created_at)
            .then_with(|| b.id.cmp(&a.id))
    });
    manifests
}

fn write_snapshot(
    store: &DeviceStore,
    app: &AppType,
    reason: ConfigBackupReason,
    pinned: bool,
    label: Option<&str>,
    captured: CapturedSnapshot,
) -> Result<ConfigBackupEntry, AppError> {
    let app_dir = app_backup_dir(store, app);
    fs::create_dir_all(&app_dir).map_err(|e| AppError::io(&app_dir, e))?;

    let id = next_backup_id(&app_dir, reason);
    let temp_dir = tempfile::Builder::new()
        .prefix(&format!(".staging-{id}-"))
        .tempdir_in(&app_dir)
        .map_err(|e| AppError::io(&app_dir, e))?;

    for record in &captured.records {
        if let (true, Some(rel)) = (record.existed, record.archive_rel_path.as_deref()) {
            let Some(bytes) = captured.bytes_by_key.get(&record.key) else {
                continue;
            };
            let dest = temp_dir.path().join(rel);
            if let Some(parent) = dest.parent() {
                fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
            }
            atomic_write_private(&dest, bytes)?;
        }
    }

    let manifest = ConfigBackupManifest {
        id: id.clone(),
        app: app.as_str().to_string(),
        reason,
        created_at: Utc::now().to_rfc3339(),
        pinned,
        label: label.map(str::to_string),
        content_digest: captured.content_digest,
        files: captured.records,
    };

    let manifest_bytes =
        serde_json::to_vec_pretty(&manifest).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write_private(&temp_dir.path().join(MANIFEST_FILENAME), &manifest_bytes)?;

    let final_dir = app_dir.join(&id);
    let staging_path = temp_dir.keep();
    if let Err(err) = fs::rename(&staging_path, &final_dir) {
        let _ = fs::remove_dir_all(&staging_path);
        return Err(AppError::io(&final_dir, err));
    }

    if !pinned {
        prune_unpinned_backups(store, app, &app_dir);
    }

    Ok(manifest_to_entry(&manifest))
}

fn prune_unpinned_backups(store: &DeviceStore, app: &AppType, app_dir: &Path) {
    let retain = effective_backup_retain_count();
    let manifests = load_app_manifests(store, app);
    let unpinned: Vec<ConfigBackupManifest> = manifests.into_iter().filter(|m| !m.pinned).collect();
    if unpinned.len() <= retain {
        return;
    }
    for old in unpinned.into_iter().skip(retain) {
        let path = app_dir.join(&old.id);
        if let Err(err) = fs::remove_dir_all(&path) {
            log::warn!(
                "Failed to prune old app config backup {}: {err}",
                path.display()
            );
        }
    }
}

/// Ensure an initial `Import` backup exists for `app` if its local config files exist on disk.
///
/// Idempotent: if an `Import` backup already exists for `app`, returns `Ok(None)` immediately
/// without touching disk.
pub fn ensure_import_backup(
    app: &AppType,
    label: Option<&str>,
) -> Result<Option<ConfigBackupEntry>, AppError> {
    if !is_auto_backup_enabled() {
        return Ok(None);
    }
    ensure_import_backup_in(&DeviceStore::for_device(), app, label)
}

pub(crate) fn ensure_import_backup_in(
    store: &DeviceStore,
    app: &AppType,
    label: Option<&str>,
) -> Result<Option<ConfigBackupEntry>, AppError> {
    let _guard = backup_ops_lock().lock_recover();
    let existing = load_app_manifests(store, app);
    if existing
        .iter()
        .any(|m| m.reason == ConfigBackupReason::Import)
    {
        return Ok(None);
    }

    let targets = resolve_app_config_targets(app)?;
    if targets.is_empty() {
        return Ok(None);
    }
    let captured = capture_targets(&targets)?;
    if !captured.any_existed {
        return Ok(None);
    }

    let entry = write_snapshot(
        store,
        app,
        ConfigBackupReason::Import,
        true,
        label,
        captured,
    )?;
    log::info!(
        "Created initial import config backup '{}' for {}",
        entry.id,
        app.as_str()
    );
    Ok(Some(entry))
}

/// Capture a backup of `app`'s current local config files before editing/saving.
///
/// Deduplicates consecutive identical states (skips if the latest snapshot already
/// matches the current files on disk, or if no config file exists yet).
pub fn backup_before_edit(
    app: &AppType,
    label: Option<&str>,
) -> Result<Option<ConfigBackupEntry>, AppError> {
    if !is_auto_backup_enabled() {
        return Ok(None);
    }
    backup_before_edit_in(&DeviceStore::for_device(), app, label)
}

pub(crate) fn backup_before_edit_in(
    store: &DeviceStore,
    app: &AppType,
    label: Option<&str>,
) -> Result<Option<ConfigBackupEntry>, AppError> {
    let _guard = backup_ops_lock().lock_recover();
    let targets = resolve_app_config_targets(app)?;
    if targets.is_empty() {
        return Ok(None);
    }
    let captured = capture_targets(&targets)?;
    if !captured.any_existed {
        return Ok(None);
    }

    let existing = load_app_manifests(store, app);
    if existing
        .iter()
        .any(|m| m.content_digest == captured.content_digest)
    {
        return Ok(None);
    }

    let entry = write_snapshot(store, app, ConfigBackupReason::Edit, false, label, captured)?;
    log::debug!(
        "Created pre-edit config backup '{}' for {}",
        entry.id,
        app.as_str()
    );
    Ok(Some(entry))
}

/// Capture a snapshot of `app`'s local config files immediately after a save action.
///
/// Unlike `backup_before_edit`, this does NOT deduplicate identical content so that
/// every explicit save creates a timestamped snapshot (subject to retention pruning),
/// except right after rolling back without making further edits.
pub fn backup_on_save(
    app: &AppType,
    label: Option<&str>,
) -> Result<Option<ConfigBackupEntry>, AppError> {
    if !is_auto_backup_enabled() {
        return Ok(None);
    }
    backup_on_save_in(&DeviceStore::for_device(), app, label)
}

pub(crate) fn backup_on_save_in(
    store: &DeviceStore,
    app: &AppType,
    label: Option<&str>,
) -> Result<Option<ConfigBackupEntry>, AppError> {
    let _guard = backup_ops_lock().lock_recover();
    let targets = resolve_app_config_targets(app)?;
    if targets.is_empty() {
        return Ok(None);
    }
    let captured = capture_targets(&targets)?;
    if !captured.any_existed {
        return Ok(None);
    }

    let key = (store.app_config_backup_dir(), app.as_str().to_string());
    if let Some(restored_digest) = last_restored_digests().lock_recover().remove(&key) {
        if restored_digest == captured.content_digest {
            return Ok(None);
        }
    }

    let entry = write_snapshot(store, app, ConfigBackupReason::Edit, false, label, captured)?;
    log::debug!(
        "Created post-save config backup '{}' for {}",
        entry.id,
        app.as_str()
    );
    Ok(Some(entry))
}

/// List config backups for one app, or across all compatible apps when `app` is `None`.
pub fn list_backups(app: Option<&AppType>) -> Result<Vec<ConfigBackupEntry>, AppError> {
    list_backups_in(&DeviceStore::for_device(), app)
}

pub(crate) fn list_backups_in(
    store: &DeviceStore,
    app: Option<&AppType>,
) -> Result<Vec<ConfigBackupEntry>, AppError> {
    let _guard = backup_ops_lock().lock_recover();
    let mut entries = Vec::new();
    match app {
        Some(target_app) => {
            for manifest in load_app_manifests(store, target_app) {
                entries.push(manifest_to_entry(&manifest));
            }
        }
        None => {
            for app_type in AppType::all() {
                for manifest in load_app_manifests(store, &app_type) {
                    entries.push(manifest_to_entry(&manifest));
                }
            }
            entries.sort_by(|a, b| {
                b.created_at
                    .cmp(&a.created_at)
                    .then_with(|| b.id.cmp(&a.id))
            });
        }
    }
    Ok(entries)
}

/// Preview the contents of a backup snapshot.
pub fn preview_backup(app: &AppType, backup_id: &str) -> Result<ConfigBackupPreview, AppError> {
    preview_backup_in(&DeviceStore::for_device(), app, backup_id)
}

pub(crate) fn preview_backup_in(
    store: &DeviceStore,
    app: &AppType,
    backup_id: &str,
) -> Result<ConfigBackupPreview, AppError> {
    validate_backup_id(backup_id)?;
    let _guard = backup_ops_lock().lock_recover();
    let snapshot_dir = app_backup_dir(store, app).join(backup_id);
    if !snapshot_dir.exists() {
        return Err(AppError::localized(
            "config_backup.not_found",
            format!("找不到配置备份: {backup_id}"),
            format!("Config backup not found: {backup_id}"),
        ));
    }

    let manifest = read_manifest(&snapshot_dir)?;
    if manifest.app != app.as_str() {
        return Err(AppError::InvalidInput(format!(
            "备份 {backup_id} 不属于应用 {}",
            app.as_str()
        )));
    }

    let entry = manifest_to_entry(&manifest);
    let mut files = Vec::with_capacity(manifest.files.len());
    for record in &manifest.files {
        if !record.existed {
            files.push(ConfigBackupFilePreview {
                key: record.key.clone(),
                original_path: record.original_path.clone(),
                existed: false,
                size_bytes: 0,
                content: None,
                truncated: false,
                binary: false,
            });
            continue;
        }
        let bytes = read_archived_file(&snapshot_dir, record)?;
        let truncated = bytes.len() > MAX_PREVIEW_BYTES;
        let slice = if truncated {
            let end = floor_utf8_boundary(&bytes, MAX_PREVIEW_BYTES);
            &bytes[..end]
        } else {
            bytes.as_slice()
        };
        match String::from_utf8(slice.to_vec()) {
            Ok(text) => files.push(ConfigBackupFilePreview {
                key: record.key.clone(),
                original_path: record.original_path.clone(),
                existed: true,
                size_bytes: record.size_bytes,
                content: Some(text),
                truncated,
                binary: false,
            }),
            Err(_) => files.push(ConfigBackupFilePreview {
                key: record.key.clone(),
                original_path: record.original_path.clone(),
                existed: true,
                size_bytes: record.size_bytes,
                content: None,
                truncated: false,
                binary: true,
            }),
        }
    }

    Ok(ConfigBackupPreview { entry, files })
}

fn read_archived_file(
    snapshot_dir: &Path,
    record: &ConfigBackupFileRecord,
) -> Result<Vec<u8>, AppError> {
    if !is_safe_rel_key(&record.key) {
        return Err(AppError::InvalidInput(format!(
            "备份文件键非法: {}",
            record.key
        )));
    }
    let rel = record
        .archive_rel_path
        .as_deref()
        .ok_or_else(|| AppError::Config(format!("备份文件 {} 缺少归档路径", record.key)))?;
    if !is_safe_rel_key(rel) {
        return Err(AppError::InvalidInput(format!("备份归档路径非法: {rel}")));
    }
    let archive_path = snapshot_dir.join(rel);
    let bytes = fs::read(&archive_path).map_err(|e| AppError::io(&archive_path, e))?;
    if let Some(expected_hash) = &record.sha256 {
        let actual_hash = sha256_hex(&bytes);
        if &actual_hash != expected_hash {
            return Err(AppError::Config(format!(
                "备份文件 {} 校验和不一致，可能已损坏",
                record.key
            )));
        }
    }
    Ok(bytes)
}

/// Roll back an app's local config files to the specified backup snapshot.
///
/// Automatically creates a `Rollback` safety snapshot of the current config files
/// before overwriting them.
pub fn restore_backup(
    state: Option<&AppState>,
    app: &AppType,
    backup_id: &str,
) -> Result<ConfigBackupRestoreResult, AppError> {
    restore_backup_in(&DeviceStore::for_device(), state, app, backup_id)
}

pub(crate) fn restore_backup_in(
    store: &DeviceStore,
    state: Option<&AppState>,
    app: &AppType,
    backup_id: &str,
) -> Result<ConfigBackupRestoreResult, AppError> {
    validate_backup_id(backup_id)?;

    // Acquire route/switch lock first, then app write lock, matching ProviderService order.
    let _route_guard = match state {
        Some(state) if app.spec().route_mode => {
            Some(crate::mode::controller::lock_settled_blocking(state, app)?)
        }
        _ => None,
    };
    let _native_guard = match state {
        Some(state) if !app.spec().route_mode => Some(futures::executor::block_on(
            state.proxy_service.lock_switch_for_app(app.as_str()),
        )),
        _ => None,
    };
    let _app_write_guard = lock_app(app.as_str());
    let _backup_guard = backup_ops_lock().lock_recover();

    let snapshot_dir = app_backup_dir(store, app).join(backup_id);
    if !snapshot_dir.exists() {
        return Err(AppError::localized(
            "config_backup.not_found",
            format!("找不到配置备份: {backup_id}"),
            format!("Config backup not found: {backup_id}"),
        ));
    }

    let manifest = read_manifest(&snapshot_dir)?;
    if manifest.app != app.as_str() {
        return Err(AppError::InvalidInput(format!(
            "备份 {backup_id} 不属于应用 {}",
            app.as_str()
        )));
    }

    // Load and verify every archived file in memory before mutating anything on disk.
    struct RestoreAction {
        key: String,
        dest: PathBuf,
        private: bool,
        bytes: Option<Vec<u8>>,
    }
    let mut actions = Vec::with_capacity(manifest.files.len());
    for record in &manifest.files {
        let Some((dest, default_private)) = resolve_target_path_for_key(app, &record.key)? else {
            return Err(AppError::InvalidInput(format!(
                "无法解析应用 {} 的配置键 {}",
                app.as_str(),
                record.key
            )));
        };
        let bytes = if record.existed {
            Some(read_archived_file(&snapshot_dir, record)?)
        } else {
            None
        };
        actions.push(RestoreAction {
            key: record.key.clone(),
            dest,
            private: record.private || default_private,
            bytes,
        });
    }

    // Rolling back restores a previously saved snapshot directly without creating another backup.
    let safety_backup_id: Option<String> = None;
    last_restored_digests().lock_recover().insert(
        (store.app_config_backup_dir(), app.as_str().to_string()),
        manifest.content_digest.clone(),
    );

    let mut restored_files = Vec::new();
    for action in actions {
        match action.bytes {
            Some(bytes) => {
                if let Some(parent) = action.dest.parent() {
                    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
                }
                if action.private {
                    atomic_write_private(&action.dest, &bytes)?;
                } else {
                    atomic_write(&action.dest, &bytes)?;
                }
                restored_files.push(action.key);
            }
            None => {
                if action.dest.exists() {
                    fs::remove_file(&action.dest).map_err(|e| AppError::io(&action.dest, e))?;
                    restored_files.push(action.key);
                }
            }
        }
    }

    // Release locks before best-effort DB re-sync for additive apps.
    drop(_backup_guard);
    drop(_app_write_guard);
    drop(_native_guard);
    drop(_route_guard);

    if let Some(state) = state {
        let sync_res = match app {
            AppType::OpenCode => {
                crate::services::provider::import_opencode_providers_from_live(state).map(|_| ())
            }
            AppType::OpenClaw => {
                crate::services::provider::import_openclaw_providers_from_live(state).map(|_| ())
            }
            AppType::Hermes => {
                crate::services::provider::import_hermes_providers_from_live(state).map(|_| ())
            }
            _ => Ok(()),
        };
        if let Err(err) = sync_res {
            log::warn!(
                "Best-effort DB sync after restoring {} backup failed: {err}",
                app.as_str()
            );
        }
    }

    log::info!(
        "Restored {} config backup '{}' (safety={:?}, files={:?})",
        app.as_str(),
        backup_id,
        safety_backup_id,
        restored_files
    );

    Ok(ConfigBackupRestoreResult {
        app: app.as_str().to_string(),
        restored_backup_id: backup_id.to_string(),
        safety_backup_id,
        restored_files,
    })
}

/// Delete a specific config backup snapshot.
pub fn delete_backup(app: &AppType, backup_id: &str) -> Result<(), AppError> {
    delete_backup_in(&DeviceStore::for_device(), app, backup_id)
}

pub(crate) fn delete_backup_in(
    store: &DeviceStore,
    app: &AppType,
    backup_id: &str,
) -> Result<(), AppError> {
    validate_backup_id(backup_id)?;
    let _guard = backup_ops_lock().lock_recover();
    let snapshot_dir = app_backup_dir(store, app).join(backup_id);
    if !snapshot_dir.exists() {
        return Err(AppError::localized(
            "config_backup.not_found",
            format!("找不到配置备份: {backup_id}"),
            format!("Config backup not found: {backup_id}"),
        ));
    }
    let manifest = read_manifest(&snapshot_dir)?;
    if manifest.app != app.as_str() {
        return Err(AppError::InvalidInput(format!(
            "备份 {backup_id} 不属于应用 {}",
            app.as_str()
        )));
    }
    fs::remove_dir_all(&snapshot_dir).map_err(|e| AppError::io(&snapshot_dir, e))?;
    Ok(())
}

/// Summary statistics for `backup_storage.rs`.
pub(crate) fn total_usage() -> (u64, u64, Option<std::time::SystemTime>) {
    let store = DeviceStore::for_device();
    let entries = list_backups_in(&store, None).unwrap_or_default();
    let count = entries.len() as u64;
    let size = crate::services::backup_storage::path_size(&store.app_config_backup_dir());
    let latest = entries
        .iter()
        .filter_map(|e| chrono::DateTime::parse_from_rfc3339(&e.created_at).ok())
        .max()
        .map(std::time::SystemTime::from);
    (count, size, latest)
}

/// Ensure `app_type` string parses cleanly.
pub(crate) fn parse_app_type(app: &str) -> Result<AppType, AppError> {
    AppType::from_str(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::{EnvVars, TempUserHome};
    use serial_test::serial;

    #[test]
    #[serial]
    fn import_backup_is_pinned_and_idempotent() {
        let home = TempUserHome::new();
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        let settings_path = claude_dir.join("settings.json");
        fs::write(&settings_path, "{\n  \"userKey\": \"v0\"\n}\n").unwrap();

        let first = ensure_import_backup(&AppType::Claude, Some("initial-import"))
            .unwrap()
            .expect("initial import backup should be created");
        assert_eq!(first.reason, ConfigBackupReason::Import);
        assert!(first.pinned);
        assert_eq!(first.files.len(), 1);
        assert!(first.files[0].existed);

        // Second call must be a no-op even if disk changes.
        fs::write(&settings_path, "{\n  \"userKey\": \"v1\"\n}\n").unwrap();
        let second = ensure_import_backup(&AppType::Claude, Some("initial-import")).unwrap();
        assert!(second.is_none());

        let all = list_backups(Some(&AppType::Claude)).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, first.id);
    }

    #[test]
    #[serial]
    fn edit_backup_deduplicates_prunes_non_pinned_and_preserves_import() {
        let home = TempUserHome::new();
        let _retain_guard = EnvVars::new();
        // Configure retain count = 2 via settings.
        let mut settings = crate::settings::get_settings();
        settings.backup_retain_count = Some(2);
        crate::settings::update_settings(settings).unwrap();

        let grok_dir = home.path().join(".grok");
        fs::create_dir_all(&grok_dir).unwrap();
        let config_path = grok_dir.join("config.toml");
        fs::write(&config_path, "# user comment\nversion = 0\n").unwrap();

        let import_entry = ensure_import_backup(&AppType::GrokBuild, Some("import"))
            .unwrap()
            .unwrap();

        // Same content as import -> deduplicated.
        assert!(backup_before_edit(&AppType::GrokBuild, Some("edit-0"))
            .unwrap()
            .is_none());

        // Edit 1..4 with distinct contents.
        for i in 1..=4 {
            fs::write(&config_path, format!("# user comment\nversion = {i}\n")).unwrap();
            backup_before_edit(&AppType::GrokBuild, Some(&format!("edit-{i}")))
                .unwrap()
                .expect("distinct edit should create backup");
        }

        let entries = list_backups(Some(&AppType::GrokBuild)).unwrap();
        // Should retain 1 pinned Import + 2 latest Edit backups = 3 total.
        assert_eq!(entries.len(), 3, "entries: {entries:?}");
        assert!(entries.iter().any(|e| e.id == import_entry.id && e.pinned));
        let edit_labels: Vec<_> = entries
            .iter()
            .filter(|e| e.reason == ConfigBackupReason::Edit)
            .filter_map(|e| e.label.as_deref())
            .collect();
        assert_eq!(edit_labels, vec!["edit-4", "edit-3"]);
    }

    #[test]
    #[serial]
    fn multi_file_restore_recovers_exact_bytes_creates_safety_backup_and_removes_new_files() {
        let home = TempUserHome::new();
        let codex_dir = home.path().join(".codex");
        fs::create_dir_all(&codex_dir).unwrap();
        let auth_path = codex_dir.join("auth.json");
        let config_path = codex_dir.join("config.toml");
        let catalog_path = codex_dir.join("cc-switch-model-catalog.json");

        // User originally only had auth.json and config.toml (with comments).
        fs::write(&auth_path, "{\"OPENAI_API_KEY\":\"orig\"}\n").unwrap();
        fs::write(
            &config_path,
            "# My custom codex config\nmodel = \"gpt-5\"\n",
        )
        .unwrap();

        let import_entry = ensure_import_backup(&AppType::Codex, Some("import"))
            .unwrap()
            .unwrap();

        // Simulate edits + CC Switch creating model catalog.
        fs::write(&auth_path, "{\"OPENAI_API_KEY\":\"edited\"}\n").unwrap();
        fs::write(&config_path, "model = \"broken\"\n").unwrap();
        fs::write(&catalog_path, "{\"models\":[]}\n").unwrap();

        let preview = preview_backup(&AppType::Codex, &import_entry.id).unwrap();
        assert_eq!(preview.files.len(), 3);
        assert!(preview.files[0]
            .content
            .as_deref()
            .unwrap()
            .contains("orig"));
        assert!(!preview.files[2].existed);

        let count_before_restore = list_backups(Some(&AppType::Codex)).unwrap().len();
        let restored = restore_backup(None, &AppType::Codex, &import_entry.id).unwrap();
        assert!(restored.safety_backup_id.is_none());
        assert_eq!(
            list_backups(Some(&AppType::Codex)).unwrap().len(),
            count_before_restore,
            "rollback must not create a new backup"
        );
        assert_eq!(
            fs::read_to_string(&auth_path).unwrap(),
            "{\"OPENAI_API_KEY\":\"orig\"}\n"
        );
        assert_eq!(
            fs::read_to_string(&config_path).unwrap(),
            "# My custom codex config\nmodel = \"gpt-5\"\n"
        );
        assert!(
            !catalog_path.exists(),
            "file that did not exist at import must be removed on rollback"
        );
    }

    #[test]
    #[serial]
    fn resolves_targets_for_all_sixteen_apps() {
        let _home = TempUserHome::new();
        for app in AppType::all() {
            let targets = resolve_app_config_targets(&app)
                .unwrap_or_else(|e| panic!("resolve targets for {}: {e}", app.as_str()));
            if app != AppType::ClaudeDesktop {
                assert!(
                    !targets.is_empty(),
                    "app {} must have at least one config target",
                    app.as_str()
                );
            }
            for target in &targets {
                let resolved = resolve_target_path_for_key(&app, &target.key).unwrap_or_else(|e| {
                    panic!("resolve key {} for {}: {e}", target.key, app.as_str())
                });
                assert!(
                    resolved.is_some(),
                    "key {} for {} must round-trip via resolve_target_path_for_key",
                    target.key,
                    app.as_str()
                );
            }
        }
    }

    #[test]
    #[serial]
    fn restore_rejects_corrupted_checksum_and_traversal() {
        let home = TempUserHome::new();
        let kimi_dir = home.path().join(".kimi-code");
        fs::create_dir_all(&kimi_dir).unwrap();
        let config_path = kimi_dir.join("config.toml");
        fs::write(&config_path, "default_model = \"k2\"\n").unwrap();

        let entry = ensure_import_backup(&AppType::Kimi, Some("import"))
            .unwrap()
            .unwrap();

        // Corrupt archived bytes.
        let archived = DeviceStore::for_device()
            .app_config_backup_dir()
            .join("kimi")
            .join(&entry.id)
            .join("files")
            .join("config.toml");
        fs::write(&archived, "corrupted").unwrap();

        let err = restore_backup(None, &AppType::Kimi, &entry.id).unwrap_err();
        assert!(err.to_string().contains("校验和"));

        let traversal = restore_backup(None, &AppType::Kimi, "../etc");
        assert!(traversal.is_err());
    }

    #[test]
    #[serial]
    fn real_import_and_edit_flows_create_backups_and_rollback() {
        use crate::database::Database;
        use crate::provider::Provider;
        use crate::services::ProviderService;
        use serde_json::json;
        use std::sync::Arc;

        let home = TempUserHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db);

        // 1. Claude: pre-existing settings -> import -> add/switch -> update -> rollback
        let claude_dir = home.path().join(".claude");
        fs::create_dir_all(&claude_dir).unwrap();
        let claude_settings = claude_dir.join("settings.json");
        let original_claude = "{\n  \"env\": {\n    \"ANTHROPIC_AUTH_TOKEN\": \"sk-user-orig\",\n    \"ANTHROPIC_BASE_URL\": \"https://orig.example.com\"\n  },\n  \"customFlag\": true\n}\n";
        fs::write(&claude_settings, original_claude).unwrap();

        assert!(ProviderService::import_default_config(&state, AppType::Claude).unwrap());

        let backups_after_import = list_backups(Some(&AppType::Claude)).unwrap();
        assert_eq!(backups_after_import.len(), 1);
        assert_eq!(backups_after_import[0].reason, ConfigBackupReason::Import);

        // Switch to a new provider (triggers Edit write via mode::operation)
        let p2 = Provider::with_id(
            "p2".into(),
            "P2".into(),
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-p2", "ANTHROPIC_BASE_URL": "https://p2.example.com"}}),
            None,
        );
        ProviderService::add(&state, AppType::Claude, p2, true).unwrap();
        ProviderService::switch(&state, AppType::Claude, "p2").unwrap();

        // Second edit/switch creates an Edit backup of p2's live config
        let p3 = Provider::with_id(
            "p3".into(),
            "P3".into(),
            json!({"env": {"ANTHROPIC_AUTH_TOKEN": "sk-p3", "ANTHROPIC_BASE_URL": "https://p3.example.com"}}),
            None,
        );
        ProviderService::add(&state, AppType::Claude, p3, true).unwrap();
        ProviderService::switch(&state, AppType::Claude, "p3").unwrap();

        let claude_backups = list_backups(Some(&AppType::Claude)).unwrap();
        assert!(
            claude_backups
                .iter()
                .any(|b| b.reason == ConfigBackupReason::Import),
            "Claude import backup expected"
        );
        assert!(
            claude_backups
                .iter()
                .any(|b| b.reason == ConfigBackupReason::Edit),
            "Claude edit backup expected: {claude_backups:?}"
        );

        let import_id = claude_backups
            .iter()
            .find(|b| b.reason == ConfigBackupReason::Import)
            .unwrap()
            .id
            .clone();
        restore_backup(Some(&state), &AppType::Claude, &import_id).unwrap();
        assert_eq!(
            fs::read_to_string(&claude_settings).unwrap(),
            original_claude
        );

        // 2. OpenCode: import from live + edit via set_provider
        let opencode_dir = home.path().join(".config").join("opencode");
        fs::create_dir_all(&opencode_dir).unwrap();
        let opencode_path = opencode_dir.join("opencode.jsonc");
        let original_opencode = "// user opencode comment\n{\n  \"provider\": {\n    \"custom\": {\n      \"npm\": \"@ai-sdk/openai\",\n      \"models\": { \"gpt-4o\": {} }\n    }\n  }\n}\n";
        fs::write(&opencode_path, original_opencode).unwrap();

        assert!(
            crate::services::provider::import_opencode_providers_from_live(&state).unwrap() > 0
        );
        crate::opencode_config::set_provider(
            "added",
            json!({"npm": "@ai-sdk/anthropic", "models": {"claude": {}}}),
        )
        .unwrap();
        crate::opencode_config::set_provider(
            "added",
            json!({"npm": "@ai-sdk/anthropic", "models": {"claude-2": {}}}),
        )
        .unwrap();

        let opencode_backups = list_backups(Some(&AppType::OpenCode)).unwrap();
        assert!(opencode_backups
            .iter()
            .any(|b| b.reason == ConfigBackupReason::Import));
        assert!(opencode_backups
            .iter()
            .any(|b| b.reason == ConfigBackupReason::Edit));

        // 3. WorkBuddy: pre-existing models.json -> first write creates Import -> second write creates Edit
        let wb_dir = home.path().join(".workbuddy");
        fs::create_dir_all(&wb_dir).unwrap();
        let wb_path = wb_dir.join("models.json");
        fs::write(
            &wb_path,
            "{\n  \"models\": [{ \"id\": \"user-m\", \"name\": \"User\", \"url\": \"https://u.example\", \"apiKey\": \"k\" }]\n}\n",
        )
        .unwrap();
        crate::workbuddy_config::set_provider(
            "wb1",
            &json!({
                "vendor": "Test",
                "url": "https://api.example.com/v1/chat/completions",
                "apiKey": "sk-1",
                "models": [{ "id": "m1", "name": "M1" }]
            }),
        )
        .unwrap();
        crate::workbuddy_config::set_provider(
            "wb1",
            &json!({
                "vendor": "Test",
                "url": "https://api.example.com/v1/chat/completions",
                "apiKey": "sk-2",
                "models": [{ "id": "m1", "name": "M1" }]
            }),
        )
        .unwrap();
        let wb_backups = list_backups(Some(&AppType::WorkBuddy)).unwrap();
        assert!(wb_backups
            .iter()
            .any(|b| b.reason == ConfigBackupReason::Import));
        assert!(wb_backups
            .iter()
            .any(|b| b.reason == ConfigBackupReason::Edit));
    }

    #[test]
    #[serial]
    fn save_from_editor_always_creates_backup_on_every_save() {
        use crate::database::Database;
        use crate::provider::Provider;
        use crate::services::ProviderService;
        use serde_json::json;
        use std::sync::Arc;

        let home = TempUserHome::new();
        let db = Arc::new(Database::memory().unwrap());
        let state = AppState::new(db);

        let hermes_dir = home.path().join(".hermes");
        fs::create_dir_all(&hermes_dir).unwrap();
        let hermes_cfg = hermes_dir.join("config.yaml");
        fs::write(
            &hermes_cfg,
            "model:\n  provider: orig\n  default: orig-model\n",
        )
        .unwrap();

        assert!(crate::services::provider::import_hermes_providers_from_live(&state).unwrap() > 0);
        let initial = list_backups(Some(&AppType::Hermes)).unwrap();
        assert_eq!(initial.len(), 1);
        assert_eq!(initial[0].reason, ConfigBackupReason::Import);

        // 1st save via add_from_editor must create a new backup immediately
        let p1 = Provider::with_id(
            "custom-p1".into(),
            "Custom P1".into(),
            json!({
                "model": { "provider": "custom-p1", "default": "gpt-5" },
                "custom_providers": [{
                    "name": "custom-p1",
                    "base_url": "https://api.example.com/v1",
                    "api_key": "sk-1",
                    "models": ["gpt-5"]
                }]
            }),
            None,
        );
        ProviderService::add_from_editor(&state, AppType::Hermes, p1.clone(), true, None).unwrap();
        let after_first_save = list_backups(Some(&AppType::Hermes)).unwrap();
        assert_eq!(after_first_save.len(), 2);

        // 2nd save via update_from_editor (even without changing config content) must create another backup
        ProviderService::update_from_editor(&state, AppType::Hermes, None, p1.clone(), None)
            .unwrap();
        let after_second_save = list_backups(Some(&AppType::Hermes)).unwrap();
        assert_eq!(after_second_save.len(), 3);
        assert_eq!(
            after_second_save
                .iter()
                .filter(|b| b.reason == ConfigBackupReason::Edit)
                .count(),
            2
        );

        // Rolling back to import and saving without new edits must NOT create a new backup
        let import_id = initial[0].id.clone();
        restore_backup(Some(&state), &AppType::Hermes, &import_id).unwrap();
        let after_rollback = list_backups(Some(&AppType::Hermes)).unwrap();
        assert_eq!(after_rollback.len(), 3);

        let orig_provider = ProviderService::list(&state, AppType::Hermes)
            .unwrap()
            .into_values()
            .next()
            .unwrap();
        ProviderService::update_from_editor(&state, AppType::Hermes, None, orig_provider, None)
            .unwrap();
        let after_rollback_save = list_backups(Some(&AppType::Hermes)).unwrap();
        assert_eq!(
            after_rollback_save.len(),
            3,
            "saving immediately after rollback must not create a new backup"
        );
    }
}
