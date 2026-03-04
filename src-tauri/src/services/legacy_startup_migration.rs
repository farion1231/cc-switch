use crate::error::AppError;
use crate::settings::{self, StartupItemsMode};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use uuid::Uuid;

const LEGACY_LAUNCH_AGENT_EXACT: &str = "com.ccswitch.env";
const LEGACY_LAUNCH_AGENT_CODEX_PROXY_ENV: &str = "com.ccswitch.codex-proxy-env";
const LEGACY_LAUNCH_AGENT_PREFIX: &str = "com.wousp.";
const MANIFEST_FILE_NAME: &str = "manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct FileBackupEntry {
    path: String,
    content_base64: String,
    #[serde(default)]
    executable: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct LegacyStartupBackupManifest {
    version: u32,
    backup_id: String,
    created_at: String,
    launch_agents: Vec<FileBackupEntry>,
    scripts: Vec<FileBackupEntry>,
    removed_labels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LegacyStartupMigrationResult {
    pub migrated: bool,
    pub skipped: bool,
    pub already_migrated: bool,
    pub backup_id: Option<String>,
    pub backup_path: Option<String>,
    pub removed_launch_agents: Vec<String>,
    pub removed_scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LegacyStartupRollbackResult {
    pub rolled_back: bool,
    pub backup_id: Option<String>,
    pub backup_path: Option<String>,
    pub restored_launch_agents: Vec<String>,
    pub restored_scripts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GuardianMigrationStatus {
    pub status: String,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backup_id: Option<String>,
}

fn backups_root_dir() -> PathBuf {
    crate::config::get_app_config_dir()
        .join("migrations")
        .join("backups")
}

fn manifest_path(backup_id: &str) -> PathBuf {
    backups_root_dir().join(backup_id).join(MANIFEST_FILE_NAME)
}

fn launch_agents_dir() -> PathBuf {
    crate::config::get_home_dir()
        .join("Library")
        .join("LaunchAgents")
}

fn local_bin_dir() -> PathBuf {
    crate::config::get_home_dir().join(".local").join("bin")
}

fn should_cleanup_launch_agent_file(path: &Path) -> Option<String> {
    let file_name = path.file_name()?.to_string_lossy();
    if !file_name.ends_with(".plist") {
        return None;
    }
    let label = file_name.trim_end_matches(".plist").to_string();

    if label == LEGACY_LAUNCH_AGENT_EXACT
        || label == LEGACY_LAUNCH_AGENT_CODEX_PROXY_ENV
        || label.starts_with(LEGACY_LAUNCH_AGENT_PREFIX)
    {
        Some(label)
    } else {
        None
    }
}

fn should_cleanup_script_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().map(|v| v.to_string_lossy()) else {
        return false;
    };

    file_name.starts_with("ccswitch-")
        || file_name == "codex-auth-normalize"
        || file_name.starts_with("codex-auth-normalize.")
}

fn file_to_backup_entry(path: &Path) -> Result<FileBackupEntry, AppError> {
    let bytes = fs::read(path).map_err(|e| AppError::io(path, e))?;
    #[cfg(unix)]
    let executable = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(path)
            .map(|meta| meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    };
    #[cfg(not(unix))]
    let executable = false;

    Ok(FileBackupEntry {
        path: path.to_string_lossy().to_string(),
        content_base64: STANDARD.encode(bytes),
        executable,
    })
}

#[cfg(target_os = "macos")]
fn unload_launch_agent(path: &Path, label: &str) {
    let path_str = path.to_string_lossy().to_string();
    let _ = Command::new("launchctl")
        .args(["unload", &path_str])
        .status();
    let _ = Command::new("launchctl").args(["remove", label]).status();
}

#[cfg(not(target_os = "macos"))]
fn unload_launch_agent(_path: &Path, _label: &str) {}

#[cfg(target_os = "macos")]
fn load_launch_agent(path: &Path) {
    let path_str = path.to_string_lossy().to_string();
    let _ = Command::new("launchctl").args(["load", &path_str]).status();
}

#[cfg(not(target_os = "macos"))]
fn load_launch_agent(_path: &Path) {}

fn collect_legacy_items() -> Result<(Vec<(PathBuf, String)>, Vec<PathBuf>), AppError> {
    let mut launch_agent_paths = Vec::<(PathBuf, String)>::new();
    let launch_dir = launch_agents_dir();
    if launch_dir.exists() {
        for entry in fs::read_dir(&launch_dir).map_err(|e| AppError::io(&launch_dir, e))? {
            let entry = entry.map_err(|e| AppError::IoContext {
                context: format!("读取 LaunchAgents 条目失败: {}", launch_dir.display()),
                source: e,
            })?;
            let path = entry.path();
            if let Some(label) = should_cleanup_launch_agent_file(&path) {
                launch_agent_paths.push((path, label));
            }
        }
    }

    let mut script_paths = Vec::<PathBuf>::new();
    let script_dir = local_bin_dir();
    if script_dir.exists() {
        for entry in fs::read_dir(&script_dir).map_err(|e| AppError::io(&script_dir, e))? {
            let entry = entry.map_err(|e| AppError::IoContext {
                context: format!("读取脚本目录条目失败: {}", script_dir.display()),
                source: e,
            })?;
            let path = entry.path();
            if should_cleanup_script_file(&path) {
                script_paths.push(path);
            }
        }
    }

    Ok((launch_agent_paths, script_paths))
}

fn build_backup_id() -> String {
    let ts = chrono::Utc::now().format("%Y%m%d%H%M%S").to_string();
    let suffix = Uuid::new_v4().simple().to_string();
    format!("{ts}-{}", &suffix[..8])
}

fn persist_manifest(manifest: &LegacyStartupBackupManifest) -> Result<PathBuf, AppError> {
    let path = manifest_path(&manifest.backup_id);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let text = serde_json::to_string_pretty(manifest)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&path, text).map_err(|e| AppError::io(&path, e))?;
    Ok(path)
}

fn read_manifest_by_id(
    backup_id: &str,
) -> Result<(LegacyStartupBackupManifest, PathBuf), AppError> {
    let path = manifest_path(backup_id);
    let text = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    let manifest = serde_json::from_str::<LegacyStartupBackupManifest>(&text)
        .map_err(|e| AppError::json(&path, e))?;
    Ok((manifest, path))
}

fn list_backup_ids() -> Result<Vec<String>, AppError> {
    let root = backups_root_dir();
    if !root.exists() {
        return Ok(Vec::new());
    }

    let mut ids = Vec::new();
    for entry in fs::read_dir(&root).map_err(|e| AppError::io(&root, e))? {
        let entry = entry.map_err(|e| AppError::IoContext {
            context: format!("读取备份目录失败: {}", root.display()),
            source: e,
        })?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let id = entry.file_name().to_string_lossy().to_string();
        if manifest_path(&id).exists() {
            ids.push(id);
        }
    }
    ids.sort();
    Ok(ids)
}

fn latest_backup_id() -> Result<Option<String>, AppError> {
    let mut ids = list_backup_ids()?;
    Ok(ids.pop())
}

fn resolve_manifest(
    backup_id: Option<&str>,
) -> Result<(LegacyStartupBackupManifest, String, PathBuf), AppError> {
    let selected = match backup_id {
        Some(id) if !id.trim().is_empty() => id.trim().to_string(),
        _ => {
            latest_backup_id()?.ok_or_else(|| AppError::Config("找不到可用迁移备份".to_string()))?
        }
    };

    let (manifest, path) = read_manifest_by_id(&selected)?;
    Ok((manifest, selected, path))
}

fn restore_backup_entries(
    entries: &[FileBackupEntry],
    restored_paths: &mut Vec<String>,
) -> Result<(), AppError> {
    for entry in entries {
        let path = PathBuf::from(&entry.path);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
        }

        let bytes = STANDARD
            .decode(entry.content_base64.as_bytes())
            .map_err(|e| AppError::Config(format!("解码备份失败 ({}): {e}", path.display())))?;

        fs::write(&path, bytes).map_err(|e| AppError::io(&path, e))?;

        #[cfg(unix)]
        if entry.executable {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o755));
        }

        restored_paths.push(path.to_string_lossy().to_string());
    }
    Ok(())
}

pub fn get_guardian_migration_status() -> Result<GuardianMigrationStatus, AppError> {
    let settings = settings::get_settings();
    let backup_id = latest_backup_id()?;
    let (legacy_launch_agents, legacy_scripts) = collect_legacy_items()?;
    let remains = legacy_launch_agents.len() + legacy_scripts.len();

    if settings.legacy_startup_migrated {
        if remains == 0 {
            Ok(GuardianMigrationStatus {
                status: "migrated".to_string(),
                message: if let Some(id) = &backup_id {
                    format!("startup item migration completed (backup: {id})")
                } else {
                    "startup item migration completed".to_string()
                },
                backup_id,
            })
        } else {
            Ok(GuardianMigrationStatus {
                status: "needs_attention".to_string(),
                message: format!("{} legacy startup items still present", remains),
                backup_id,
            })
        }
    } else {
        Ok(GuardianMigrationStatus {
            status: "pending".to_string(),
            message: "startup item migration pending".to_string(),
            backup_id,
        })
    }
}

pub fn migrate_legacy_startup_items_if_needed() -> Result<LegacyStartupMigrationResult, AppError> {
    migrate_legacy_startup_items(false)
}

pub fn migrate_legacy_startup_items(force: bool) -> Result<LegacyStartupMigrationResult, AppError> {
    let mut current = settings::get_settings();
    let (launch_agent_paths, script_paths) = collect_legacy_items()?;
    let has_legacy = !(launch_agent_paths.is_empty() && script_paths.is_empty());

    if current.legacy_startup_migrated && !force && !has_legacy {
        let backup_id = latest_backup_id()?;
        let backup_path = backup_id
            .as_deref()
            .map(|id| manifest_path(id).to_string_lossy().to_string());
        return Ok(LegacyStartupMigrationResult {
            skipped: true,
            already_migrated: true,
            backup_id,
            backup_path,
            ..LegacyStartupMigrationResult::default()
        });
    }

    if !has_legacy {
        let mut changed = false;
        if !current.legacy_startup_migrated {
            current.legacy_startup_migrated = true;
            changed = true;
        }
        if current.startup_items_mode != StartupItemsMode::AutoLaunch {
            current.startup_items_mode = StartupItemsMode::AutoLaunch;
            changed = true;
        }
        if changed {
            settings::update_settings(current)?;
        }

        let backup_id = latest_backup_id()?;
        let backup_path = backup_id
            .as_deref()
            .map(|id| manifest_path(id).to_string_lossy().to_string());

        return Ok(LegacyStartupMigrationResult {
            migrated: changed,
            skipped: true,
            already_migrated: !changed,
            backup_id,
            backup_path,
            ..LegacyStartupMigrationResult::default()
        });
    }

    let backup_id = build_backup_id();
    let mut manifest = LegacyStartupBackupManifest {
        version: 2,
        backup_id: backup_id.clone(),
        created_at: chrono::Utc::now().to_rfc3339(),
        ..LegacyStartupBackupManifest::default()
    };

    let mut removed_launch_agents = Vec::new();
    let mut removed_scripts = Vec::new();

    for (path, label) in &launch_agent_paths {
        if !path.exists() {
            continue;
        }
        manifest.launch_agents.push(file_to_backup_entry(path)?);
        manifest.removed_labels.push(label.clone());
        unload_launch_agent(path, label);
        fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
        removed_launch_agents.push(label.clone());
    }

    for path in &script_paths {
        if !path.exists() {
            continue;
        }
        manifest.scripts.push(file_to_backup_entry(path)?);
        fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
        removed_scripts.push(path.to_string_lossy().to_string());
    }

    let saved_manifest_path = persist_manifest(&manifest)?;

    current.legacy_startup_migrated = true;
    current.startup_items_mode = StartupItemsMode::AutoLaunch;
    settings::update_settings(current)?;

    Ok(LegacyStartupMigrationResult {
        migrated: true,
        skipped: false,
        already_migrated: false,
        backup_id: Some(backup_id),
        backup_path: Some(saved_manifest_path.to_string_lossy().to_string()),
        removed_launch_agents,
        removed_scripts,
    })
}

pub fn rollback_legacy_migration_with_backup_id(
    backup_id: Option<String>,
) -> Result<LegacyStartupRollbackResult, AppError> {
    let (manifest, selected_backup_id, path) = resolve_manifest(backup_id.as_deref())?;

    let mut restored_launch_agents = Vec::new();
    let mut restored_scripts = Vec::new();

    restore_backup_entries(&manifest.launch_agents, &mut restored_launch_agents)?;
    restore_backup_entries(&manifest.scripts, &mut restored_scripts)?;

    for item in &manifest.launch_agents {
        load_launch_agent(Path::new(&item.path));
    }

    let mut current = settings::get_settings();
    current.legacy_startup_migrated = false;
    current.startup_items_mode = StartupItemsMode::LegacyLaunchAgent;
    settings::update_settings(current)?;

    Ok(LegacyStartupRollbackResult {
        rolled_back: true,
        backup_id: Some(selected_backup_id),
        backup_path: Some(path.to_string_lossy().to_string()),
        restored_launch_agents,
        restored_scripts,
    })
}

pub fn rollback_legacy_migration() -> Result<LegacyStartupRollbackResult, AppError> {
    rollback_legacy_migration_with_backup_id(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_agent_matcher_filters_expected_labels() {
        let p1 = PathBuf::from("com.ccswitch.env.plist");
        let p1b = PathBuf::from("com.ccswitch.codex-proxy-env.plist");
        let p2 = PathBuf::from("com.wousp.foo.plist");
        let p3 = PathBuf::from("com.example.keep.plist");

        assert_eq!(
            should_cleanup_launch_agent_file(&p1).as_deref(),
            Some("com.ccswitch.env")
        );
        assert_eq!(
            should_cleanup_launch_agent_file(&p1b).as_deref(),
            Some("com.ccswitch.codex-proxy-env")
        );
        assert_eq!(
            should_cleanup_launch_agent_file(&p2).as_deref(),
            Some("com.wousp.foo")
        );
        assert!(should_cleanup_launch_agent_file(&p3).is_none());
    }

    #[test]
    fn script_matcher_filters_expected_files() {
        assert!(should_cleanup_script_file(Path::new("ccswitch-env")));
        assert!(should_cleanup_script_file(Path::new(
            "ccswitch-proxy-guard"
        )));
        assert!(should_cleanup_script_file(Path::new(
            "codex-auth-normalize"
        )));
        assert!(should_cleanup_script_file(Path::new(
            "codex-auth-normalize.sh"
        )));
        assert!(!should_cleanup_script_file(Path::new("random-script")));
    }

    #[test]
    fn backup_id_contains_dash_and_suffix() {
        let id = build_backup_id();
        assert!(id.contains('-'));
        assert!(id.len() >= 15);
    }
}
