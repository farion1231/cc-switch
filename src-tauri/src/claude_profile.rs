//! Managed Claude profile service
//!
//! Creates, synchronizes, and manages isolated Claude Code profiles
//! for the Claude launcher feature. Each managed profile
//! has its own `CLAUDE_CONFIG_DIR` with separate settings, MCP config,
//! sessions, and credentials.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::app_config::AppType;
use crate::config::{
    atomic_write, derive_provider_slug, get_claude_mcp_path, get_managed_profile_root,
    ClaudeConfigTarget,
};
use crate::database::Database;
use crate::error::AppError;
use crate::provider::Provider;
use crate::services::provider::live::{
    build_effective_settings_with_common_config, sanitize_claude_settings_for_live,
};

/// Status of a managed Claude profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ProfileStatus {
    /// Profile directory and config files are present and valid.
    Ready,
    /// Profile directory does not exist yet.
    Missing,
    /// Profile exists but the provider settings have changed since last sync.
    Stale,
    /// Last sync attempt failed.
    SyncFailed,
}

/// Result of a profile synchronization operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSyncResult {
    pub profile_dir: String,
    pub status: ProfileStatus,
    pub settings_written: bool,
    pub mcp_written: bool,
    pub error: Option<String>,
}

/// Build the default launch command for a managed profile.
///
/// The command is stored for display/copy and is also safe to execute from
/// CC Switch's terminal launcher on the current platform.
pub fn default_launch_command(profile_dir: &Path) -> String {
    launch_command_for_profile(profile_dir, false)
}

/// Return the generated launcher settings overlay path for a managed profile.
pub fn launcher_settings_overlay_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join("cc-switch-launcher-settings.json")
}

/// Whether this provider has a launcher permission mode override.
pub fn has_launcher_permission_mode(provider: &Provider) -> bool {
    provider
        .meta
        .as_ref()
        .and_then(|meta| meta.launcher_permission_mode)
        .is_some()
}

/// Build the managed profile launch command, optionally applying the
/// launcher-owned settings overlay with Claude Code's `--settings` flag.
pub fn launch_command_for_profile(profile_dir: &Path, include_launcher_overlay: bool) -> String {
    let profile = profile_dir.to_string_lossy();

    #[cfg(target_os = "windows")]
    {
        let base = format!(
            "set \"CLAUDE_CONFIG_DIR={}\" && claude",
            escape_windows_batch_value(&profile)
        );
        if include_launcher_overlay {
            let overlay = launcher_settings_overlay_path(profile_dir);
            let overlay = overlay.to_string_lossy();
            return format!(
                "{} --settings \"{}\"",
                base,
                escape_windows_batch_value(&overlay)
            );
        }
        base
    }

    #[cfg(not(target_os = "windows"))]
    {
        let base = format!("CLAUDE_CONFIG_DIR={} claude", shell_single_quote(&profile));
        if include_launcher_overlay {
            let overlay = launcher_settings_overlay_path(profile_dir);
            let overlay = overlay.to_string_lossy();
            return format!("{} --settings {}", base, shell_single_quote(&overlay));
        }
        base
    }
}

/// Build the default launch command for a provider's managed profile.
pub fn default_launch_command_for_provider(provider: &Provider, profile_dir: &Path) -> String {
    launch_command_for_profile(profile_dir, has_launcher_permission_mode(provider))
}

#[cfg(target_os = "windows")]
fn escape_windows_batch_value(value: &str) -> String {
    value
        .replace('^', "^^")
        .replace('%', "%%")
        .replace('&', "^&")
        .replace('|', "^|")
        .replace('<', "^<")
        .replace('>', "^>")
        .replace('(', "^(")
        .replace(')', "^)")
}

#[cfg(not(target_os = "windows"))]
fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

fn is_managed_default_launch_command(
    command: &str,
    profile_dir: &Path,
    shortcut_name: Option<&str>,
) -> bool {
    if shortcut_name == Some(command) {
        return true;
    }
    if command == default_launch_command(profile_dir) {
        return true;
    }
    command.contains("cc-switch-launcher-settings.json")
}

/// Resolve the command CC Switch should run when it opens a Claude terminal.
///
/// Shell shortcuts are optional user conveniences and may live in a directory
/// that is not on PATH. If metadata still contains a shortcut command from an
/// older build, fall back to the explicit profile launch command so the app's
/// own launch flow remains reliable.
pub fn terminal_launch_command(provider: &Provider, profile_dir: &Path) -> String {
    let default_command = default_launch_command_for_provider(provider, profile_dir);
    let Some(meta) = provider.meta.as_ref() else {
        return default_command;
    };

    let Some(command) = meta
        .launch_command
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
    else {
        return default_command;
    };

    if is_managed_default_launch_command(command, profile_dir, meta.shortcut_name.as_deref()) {
        return default_command;
    }

    command.to_string()
}

/// Resolve the `ClaudeConfigTarget` for a given provider.
pub fn resolve_target(_db: &Database, provider: &Provider) -> ClaudeConfigTarget {
    let slug = derive_provider_slug(&provider.id);
    let path_override = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_profile_path.as_ref())
        .map(|p| PathBuf::from(p));

    ClaudeConfigTarget::ManagedProfile {
        slug,
        path_override,
    }
}

/// Ensure the managed profile directory exists and return its path.
fn ensure_profile_dir(target: &ClaudeConfigTarget) -> Result<PathBuf, AppError> {
    let dir = target.config_dir();
    if !dir.exists() {
        fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
        log::info!("Created managed profile directory: {}", dir.display());
    }
    Ok(dir)
}

/// Synchronize provider settings into the managed profile.
///
/// This reuses the existing effective-settings + common-config pipeline
/// from `write_live_with_common_config`, then writes to the profile target
/// instead of the global Claude config.
pub fn sync_settings_to_profile(
    db: &Database,
    provider: &Provider,
    target: &ClaudeConfigTarget,
) -> Result<bool, AppError> {
    let effective = build_effective_settings_with_common_config(db, &AppType::Claude, provider)?;
    let sanitized = sanitize_claude_settings_for_live(&effective);

    let settings_path = target.settings_path();
    let parent = settings_path
        .parent()
        .ok_or_else(|| AppError::Config("Invalid settings path".into()))?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;

    let json_str = serde_json::to_string_pretty(&sanitized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&settings_path, json_str.as_bytes())?;

    log::info!("Synced settings to profile: {}", settings_path.display());
    Ok(true)
}

/// Synchronize the launcher-owned settings overlay for permission mode.
///
/// The overlay intentionally contains only the setting owned by the launcher
/// workflow. Provider/common-config permission rules stay in `settings.json`.
pub fn sync_launcher_settings_overlay(
    provider: &Provider,
    target: &ClaudeConfigTarget,
) -> Result<bool, AppError> {
    let profile_dir = target.config_dir();
    let overlay_path = launcher_settings_overlay_path(&profile_dir);
    let mode = provider
        .meta
        .as_ref()
        .and_then(|meta| meta.launcher_permission_mode);

    let Some(mode) = mode else {
        if overlay_path.exists() {
            fs::remove_file(&overlay_path).map_err(|e| AppError::io(&overlay_path, e))?;
            log::info!(
                "Removed launcher settings overlay: {}",
                overlay_path.display()
            );
            return Ok(true);
        }
        return Ok(false);
    };

    let parent = overlay_path
        .parent()
        .ok_or_else(|| AppError::Config("Invalid launcher settings overlay path".into()))?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;

    let overlay = json!({
        "permissions": {
            "defaultMode": mode.as_str()
        }
    });
    let json_str = serde_json::to_string_pretty(&overlay)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&overlay_path, json_str.as_bytes())?;

    log::info!(
        "Synced launcher settings overlay: {}",
        overlay_path.display()
    );
    Ok(true)
}

/// Synchronize enabled Claude MCP servers into the managed profile's
/// `.claude.json`, preserving unrelated keys.
pub fn sync_mcp_to_profile(_db: &Database, target: &ClaudeConfigTarget) -> Result<bool, AppError> {
    let mcp_path = target.mcp_path();

    // Read existing profile state (preserve unrelated keys)
    let mut root: Value = if mcp_path.exists() {
        let content = fs::read_to_string(&mcp_path).map_err(|e| AppError::io(&mcp_path, e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    // Get the current Claude MCP servers from the global MCP config
    let enabled_servers = crate::claude_mcp::read_mcp_servers_map()?;

    // Update only the mcpServers key
    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Profile .claude.json root must be object".into()))?;

    if enabled_servers.is_empty() {
        obj.remove("mcpServers");
    } else {
        let mut out: Map<String, Value> = Map::new();
        for (id, spec) in &enabled_servers {
            let mut server_obj = if let Some(map) = spec.as_object() {
                map.clone()
            } else {
                continue;
            };
            // Remove CC Switch UI-only fields
            server_obj.remove("enabled");
            server_obj.remove("source");
            server_obj.remove("id");
            server_obj.remove("name");
            server_obj.remove("description");
            server_obj.remove("tags");
            server_obj.remove("homepage");
            server_obj.remove("docs");
            if let Some(server_val) = server_obj.remove("server") {
                if let Some(server_map) = server_val.as_object() {
                    server_obj = server_map.clone();
                }
            }
            out.insert(id.clone(), Value::Object(server_obj));
        }
        obj.insert("mcpServers".into(), Value::Object(out));
    }

    let parent = mcp_path
        .parent()
        .ok_or_else(|| AppError::Config("Invalid MCP path".into()))?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;

    let json_str =
        serde_json::to_string_pretty(&root).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&mcp_path, json_str.as_bytes())?;

    log::info!("Synced MCP to profile: {}", mcp_path.display());
    Ok(true)
}

/// Apply Claude onboarding integration setting to the managed profile.
///
/// If the global option to set `hasCompletedOnboarding` is enabled,
/// apply it to the profile's `.claude.json` as well.
pub fn sync_onboarding_to_profile(target: &ClaudeConfigTarget) -> Result<bool, AppError> {
    // Check if the global onboarding skip is enabled
    let global_mcp_path = get_claude_mcp_path();
    if !global_mcp_path.exists() {
        return Ok(false);
    }

    let global_content =
        fs::read_to_string(&global_mcp_path).map_err(|e| AppError::io(&global_mcp_path, e))?;
    let global_root: Value = serde_json::from_str(&global_content).unwrap_or_else(|_| json!({}));

    let has_onboarding = global_root
        .get("hasCompletedOnboarding")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if !has_onboarding {
        return Ok(false);
    }

    // Apply to profile
    let mcp_path = target.mcp_path();
    let mut root: Value = if mcp_path.exists() {
        let content = fs::read_to_string(&mcp_path).map_err(|e| AppError::io(&mcp_path, e))?;
        serde_json::from_str(&content).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };

    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Profile .claude.json root must be object".into()))?;

    let already = obj
        .get("hasCompletedOnboarding")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    if already {
        return Ok(false);
    }

    obj.insert("hasCompletedOnboarding".into(), Value::Bool(true));

    let parent = mcp_path
        .parent()
        .ok_or_else(|| AppError::Config("Invalid MCP path".into()))?;
    fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;

    let json_str =
        serde_json::to_string_pretty(&root).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(&mcp_path, json_str.as_bytes())?;

    Ok(true)
}

/// Full profile synchronization: settings + MCP + onboarding.
///
/// Creates the profile directory on demand if it doesn't exist.
pub fn sync_profile(db: &Database, provider: &Provider) -> Result<ProfileSyncResult, AppError> {
    let target = resolve_target(db, provider);

    // Ensure directory exists
    let profile_dir = ensure_profile_dir(&target)?;

    // Sync settings
    let settings_written = match sync_settings_to_profile(db, provider, &target) {
        Ok(written) => written,
        Err(e) => {
            return Ok(ProfileSyncResult {
                profile_dir: profile_dir.to_string_lossy().to_string(),
                status: ProfileStatus::SyncFailed,
                settings_written: false,
                mcp_written: false,
                error: Some(e.to_string()),
            });
        }
    };

    // Sync launcher-owned permission-mode overlay.
    if let Err(e) = sync_launcher_settings_overlay(provider, &target) {
        return Ok(ProfileSyncResult {
            profile_dir: profile_dir.to_string_lossy().to_string(),
            status: ProfileStatus::SyncFailed,
            settings_written,
            mcp_written: false,
            error: Some(format!("Launcher settings sync failed: {e}")),
        });
    }

    // Sync MCP
    let mcp_written = match sync_mcp_to_profile(db, &target) {
        Ok(written) => written,
        Err(e) => {
            return Ok(ProfileSyncResult {
                profile_dir: profile_dir.to_string_lossy().to_string(),
                status: ProfileStatus::SyncFailed,
                settings_written,
                mcp_written: false,
                error: Some(format!("MCP sync failed: {e}")),
            });
        }
    };

    // Sync onboarding
    let _ = sync_onboarding_to_profile(&target);

    // TODO: Sync skills (task 2.4) — will be implemented when skill service
    // supports writing to arbitrary Claude config targets.

    Ok(ProfileSyncResult {
        profile_dir: profile_dir.to_string_lossy().to_string(),
        status: ProfileStatus::Ready,
        settings_written,
        mcp_written,
        error: None,
    })
}

/// Synchronize a profile and persist derived provider metadata needed by the UI
/// and launch flow.
pub fn sync_profile_and_update_metadata(
    db: &Database,
    provider: &Provider,
) -> Result<(ProfileSyncResult, Provider), AppError> {
    let result = sync_profile(db, provider)?;
    let mut updated = provider.clone();

    if result.status == ProfileStatus::Ready {
        let profile_dir = PathBuf::from(&result.profile_dir);
        let default_command = default_launch_command_for_provider(&updated, &profile_dir);

        let meta = updated.meta.get_or_insert_with(Default::default);
        if meta.managed_profile_path.is_none() {
            meta.managed_profile_path = Some(profile_dir.to_string_lossy().to_string());
        }
        let launch_is_missing = meta
            .launch_command
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_none();
        let launch_is_managed_default = meta
            .launch_command
            .as_deref()
            .map(|command| {
                is_managed_default_launch_command(
                    command,
                    &profile_dir,
                    meta.shortcut_name.as_deref(),
                )
            })
            .unwrap_or(true);
        if launch_is_missing || launch_is_managed_default {
            meta.launch_command = Some(default_command);
        }

        db.save_provider("claude", &updated)?;
    }

    Ok((result, updated))
}

/// Check the current status of a managed profile.
pub fn get_profile_status(provider: &Provider) -> ProfileStatus {
    let slug = derive_provider_slug(&provider.id);
    let path_override = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_profile_path.as_ref())
        .map(PathBuf::from);

    let target = ClaudeConfigTarget::ManagedProfile {
        slug,
        path_override,
    };

    let dir = target.config_dir();
    if !dir.exists() {
        return ProfileStatus::Missing;
    }

    let settings = target.settings_path();
    let mcp = target.mcp_path();

    if !settings.exists() {
        return ProfileStatus::Missing;
    }

    // Check if the profile is stale by comparing settings content
    // For now, if the directory and settings exist, we consider it ready.
    // Staleness detection will be refined when the frontend provides
    // explicit sync/repair actions.
    let _ = mcp; // MCP file is optional

    ProfileStatus::Ready
}

/// Remove a managed profile directory and all its contents.
pub fn remove_profile(provider: &Provider) -> Result<bool, AppError> {
    let slug = derive_provider_slug(&provider.id);
    let path_override = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_profile_path.as_ref())
        .map(PathBuf::from);

    let target = ClaudeConfigTarget::ManagedProfile {
        slug,
        path_override,
    };

    let dir = target.config_dir();
    if !dir.exists() {
        return Ok(false);
    }

    // Safety check: only remove directories under the managed profile root
    let root = get_managed_profile_root();
    if !dir.starts_with(&root) {
        log::warn!(
            "Refusing to remove profile directory outside managed root: {}",
            dir.display()
        );
        return Ok(false);
    }

    fs::remove_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    log::info!("Removed managed profile: {}", dir.display());
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeLauncherPermissionMode, Provider, ProviderMeta};
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn resolve_target_derives_slug_from_provider_id() {
        let provider = Provider::with_id(
            "universal-claude-kimi".to_string(),
            "Kimi".to_string(),
            json!({}),
            None,
        );
        let db = Database::memory().unwrap();
        let target = resolve_target(&db, &provider);
        match target {
            ClaudeConfigTarget::ManagedProfile { slug, .. } => {
                assert_eq!(slug, "kimi");
            }
            _ => panic!("expected ManagedProfile"),
        }
    }

    #[test]
    fn get_profile_status_missing_when_dir_absent() {
        let provider = Provider::with_id(
            "test-nonexistent-profile".to_string(),
            "Test".to_string(),
            json!({}),
            None,
        );
        assert_eq!(get_profile_status(&provider), ProfileStatus::Missing);
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn default_launch_command_quotes_profile_path() {
        let command = default_launch_command(Path::new("/tmp/profile O'Brien"));

        assert_eq!(
            command,
            "CLAUDE_CONFIG_DIR='/tmp/profile O'\"'\"'Brien' claude"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn terminal_launch_command_ignores_shortcut_metadata() {
        let profile_dir = Path::new("/tmp/profile");
        let mut provider = Provider::with_id(
            "universal-claude-kimi".to_string(),
            "Kimi".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            launch_command: Some("claude-kimi".to_string()),
            shortcut_name: Some("claude-kimi".to_string()),
            ..Default::default()
        });

        assert_eq!(
            terminal_launch_command(&provider, profile_dir),
            "CLAUDE_CONFIG_DIR='/tmp/profile' claude"
        );
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn terminal_launch_command_applies_launcher_settings_overlay() {
        let profile_dir = Path::new("/tmp/profile");
        let mut provider = Provider::with_id(
            "universal-claude-kimi".to_string(),
            "Kimi".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            launcher_permission_mode: Some(ClaudeLauncherPermissionMode::Plan),
            ..Default::default()
        });

        assert_eq!(
            terminal_launch_command(&provider, profile_dir),
            "CLAUDE_CONFIG_DIR='/tmp/profile' claude --settings '/tmp/profile/cc-switch-launcher-settings.json'"
        );
    }

    #[test]
    fn launcher_settings_overlay_path_is_profile_local() {
        assert_eq!(
            launcher_settings_overlay_path(Path::new("/tmp/profile")),
            PathBuf::from("/tmp/profile/cc-switch-launcher-settings.json")
        );
    }

    #[test]
    fn sync_launcher_settings_overlay_writes_only_default_mode() {
        let temp = tempdir().expect("tempdir");
        let target = ClaudeConfigTarget::ManagedProfile {
            slug: "test".to_string(),
            path_override: Some(temp.path().to_path_buf()),
        };
        let mut provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({
                "env": { "ANTHROPIC_API_KEY": "sk-secret" },
                "permissions": { "allow": ["Bash"] }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            launcher_permission_mode: Some(ClaudeLauncherPermissionMode::AcceptEdits),
            ..Default::default()
        });

        assert!(sync_launcher_settings_overlay(&provider, &target).unwrap());
        let overlay_path = launcher_settings_overlay_path(temp.path());
        let overlay: Value =
            serde_json::from_str(&fs::read_to_string(overlay_path).unwrap()).unwrap();

        assert_eq!(
            overlay
                .get("permissions")
                .and_then(|value| value.get("defaultMode"))
                .and_then(Value::as_str),
            Some("acceptEdits")
        );
        assert!(overlay.get("env").is_none());
        assert!(!overlay.to_string().contains("sk-secret"));
        assert!(overlay
            .get("permissions")
            .and_then(|value| value.get("allow"))
            .is_none());
    }

    #[test]
    fn sync_launcher_settings_overlay_removes_when_unset() {
        let temp = tempdir().expect("tempdir");
        let target = ClaudeConfigTarget::ManagedProfile {
            slug: "test".to_string(),
            path_override: Some(temp.path().to_path_buf()),
        };
        let overlay_path = launcher_settings_overlay_path(temp.path());
        fs::write(&overlay_path, "{}").unwrap();

        let provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({}),
            None,
        );

        assert!(sync_launcher_settings_overlay(&provider, &target).unwrap());
        assert!(!overlay_path.exists());
    }

    #[test]
    fn sync_profile_preserves_permission_rules_in_settings() {
        let temp = tempdir().expect("tempdir");
        let mut provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({
                "permissions": {
                    "allow": ["Bash"],
                    "ask": ["Edit"],
                    "deny": ["WebFetch"]
                }
            }),
            None,
        );
        provider.meta = Some(ProviderMeta {
            managed_profile_path: Some(temp.path().to_string_lossy().to_string()),
            launcher_permission_mode: Some(ClaudeLauncherPermissionMode::Plan),
            ..Default::default()
        });

        let db = Database::memory().unwrap();
        let target = resolve_target(&db, &provider);
        assert!(sync_settings_to_profile(&db, &provider, &target).unwrap());
        let settings: Value =
            serde_json::from_str(&fs::read_to_string(temp.path().join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(settings["permissions"]["allow"], json!(["Bash"]));
        assert_eq!(settings["permissions"]["ask"], json!(["Edit"]));
        assert_eq!(settings["permissions"]["deny"], json!(["WebFetch"]));
        assert!(settings["permissions"].get("defaultMode").is_none());
    }
}
