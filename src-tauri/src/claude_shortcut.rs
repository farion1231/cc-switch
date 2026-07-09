//! Shortcut command service for managed Claude profiles
//!
//! Manages provider-specific command wrappers (e.g. `claude-kimi`) that
//! launch Claude Code with the correct `CLAUDE_CONFIG_DIR` profile.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ClaudeConfigTarget;
use crate::error::AppError;
use crate::provider::Provider;

/// CC Switch ownership marker written inside wrapper scripts.
const OWNERSHIP_MARKER_PREFIX: &str = "# cc-switch:managed:";
/// Default user-writable bin directory for shortcut installation.
pub fn get_user_bin_dir() -> PathBuf {
    crate::config::get_home_dir().join(".local/bin")
}

/// Generate a launcher command name from a provider display name.
///
/// Produces names like `claude-kimi`, `claude-deepseek`, etc.
pub fn generate_shortcut_name(provider_name: &str) -> String {
    let slug = crate::config::slugify(provider_name);
    format!("claude-{slug}")
}

/// Resolve the effective shortcut name for a provider.
/// Uses the stored name if available, otherwise generates one.
fn resolve_shortcut_name(provider: &Provider) -> String {
    let default_name = generate_shortcut_name(&provider.name);
    let legacy_id_name = legacy_id_shortcut_name(provider);

    provider
        .meta
        .as_ref()
        .and_then(|m| m.shortcut_name.clone())
        .filter(|name| name != &legacy_id_name)
        .unwrap_or(default_name)
}

/// Previous launcher drafts generated aliases from internal provider IDs.
/// Treat that exact value as an auto-generated legacy default so the visible
/// default can move to `claude-<provider-name>` without overwriting custom names.
fn legacy_id_shortcut_name(provider: &Provider) -> String {
    format!(
        "claude-{}",
        crate::config::derive_provider_slug(&provider.id)
    )
}

/// Validate that a shortcut name is safe for use as a command.
pub fn validate_shortcut_name(name: &str) -> Result<(), AppError> {
    if name.is_empty() {
        return Err(AppError::InvalidInput(
            "Shortcut name cannot be empty".into(),
        ));
    }
    if !name
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.')
    {
        return Err(AppError::InvalidInput(
            "Shortcut name may only contain ASCII letters, numbers, dots, underscores, and hyphens"
                .into(),
        ));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(AppError::InvalidInput(
            "Shortcut name cannot contain path separators".into(),
        ));
    }
    if name.starts_with('-') {
        return Err(AppError::InvalidInput(
            "Shortcut name cannot start with a hyphen".into(),
        ));
    }
    if name == "." || name == ".." {
        return Err(AppError::InvalidInput(
            "Shortcut name cannot be '.' or '..'".into(),
        ));
    }
    Ok(())
}

/// Render a wrapper script for a managed profile.
///
/// The script sets `CLAUDE_CONFIG_DIR` and executes `claude`,
/// forwarding all arguments. It does not contain provider secrets.
pub fn render_wrapper_script(profile_dir: &Path, provider_id: &str) -> String {
    let profile_path = profile_dir.to_string_lossy();
    let profile_path_quoted = shell_single_quote(&profile_path);
    let slug = crate::config::derive_provider_slug(provider_id);
    let overlay_name = crate::claude_profile::launcher_settings_overlay_path(profile_dir)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("cc-switch-launcher-settings.json")
        .to_string();

    format!(
        r#"#!/usr/bin/env bash
# cc-switch:managed:{slug}
# Managed by CC Switch — do not edit manually.
# Profile: {profile_path}

export CLAUDE_CONFIG_DIR={profile_path_quoted}
cc_switch_launcher_settings="$CLAUDE_CONFIG_DIR/{overlay_name}"
if [ -f "$cc_switch_launcher_settings" ]; then
  exec claude --settings "$cc_switch_launcher_settings" "$@"
fi
exec claude "$@"
"#
    )
}

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\"'\"'"))
}

/// Parse the ownership marker from a wrapper script.
///
/// Returns `Some(provider_slug)` if the file is a managed CC Switch shortcut,
/// or `None` if it is not managed or cannot be read.
pub fn parse_ownership_marker(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(OWNERSHIP_MARKER_PREFIX) {
            return Some(rest.to_string());
        }
        // Stop searching after the first few lines (marker is always near top)
        if !trimmed.starts_with('#') && !trimmed.is_empty() {
            break;
        }
    }
    None
}

/// Check if a file at the given path is managed by CC Switch for the given provider slug.
pub fn is_managed_shortcut(path: &Path, expected_slug: &str) -> Result<bool, AppError> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    match parse_ownership_marker(&content) {
        Some(slug) => Ok(slug == expected_slug),
        None => Ok(false),
    }
}

fn wrapper_supports_launcher_settings_overlay(content: &str) -> bool {
    content.contains("cc-switch-launcher-settings.json")
        && content.contains("--settings")
        && content.contains("\"$@\"")
}

/// Status of a shortcut installation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ShortcutStatus {
    /// Shortcut is installed and up-to-date.
    Installed,
    /// Shortcut exists but points to a different profile path.
    Stale,
    /// Shortcut file is missing but metadata says it should be there.
    Missing,
    /// Target path is occupied by an unmanaged command.
    Conflict,
}

/// Detailed shortcut status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutInfo {
    pub name: String,
    pub target_path: String,
    pub status: ShortcutStatus,
    /// The profile directory the shortcut currently points to, if parseable.
    pub current_profile_dir: Option<String>,
}

/// Get the expected shortcut path for a given name and target directory.
pub fn shortcut_path(name: &str, target_dir: &Path) -> PathBuf {
    target_dir.join(name)
}

/// Check shortcut status.
pub fn get_shortcut_status(
    provider: &Provider,
    target_dir: &Path,
) -> Result<ShortcutInfo, AppError> {
    let slug = crate::config::derive_provider_slug(&provider.id);
    let name = resolve_shortcut_name(provider);

    validate_shortcut_name(&name)?;
    let path = shortcut_path(&name, target_dir);

    if !path.exists() {
        return Ok(ShortcutInfo {
            name: name.to_string(),
            target_path: path.to_string_lossy().to_string(),
            status: ShortcutStatus::Missing,
            current_profile_dir: None,
        });
    }

    let content = fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;

    // Check ownership
    let marker_slug = parse_ownership_marker(&content);
    match marker_slug {
        Some(owner_slug) if owner_slug == slug => {
            // Managed by us — check if profile path is current
            let expected_profile = resolve_profile_dir(provider);
            let current_profile = extract_profile_dir_from_script(&content);
            let has_current_profile =
                current_profile.as_deref() == Some(expected_profile.to_str().unwrap_or(""));
            let overlay_required = provider
                .meta
                .as_ref()
                .and_then(|meta| meta.launcher_permission_mode)
                .is_some();
            let overlay_supported =
                !overlay_required || wrapper_supports_launcher_settings_overlay(&content);
            let status = if has_current_profile && overlay_supported {
                ShortcutStatus::Installed
            } else {
                ShortcutStatus::Stale
            };
            Ok(ShortcutInfo {
                name: name.to_string(),
                target_path: path.to_string_lossy().to_string(),
                status,
                current_profile_dir: current_profile,
            })
        }
        Some(_) => {
            // Managed by CC Switch but for a different provider
            Ok(ShortcutInfo {
                name: name.to_string(),
                target_path: path.to_string_lossy().to_string(),
                status: ShortcutStatus::Conflict,
                current_profile_dir: None,
            })
        }
        None => {
            // Not managed by CC Switch
            Ok(ShortcutInfo {
                name: name.to_string(),
                target_path: path.to_string_lossy().to_string(),
                status: ShortcutStatus::Conflict,
                current_profile_dir: None,
            })
        }
    }
}

/// Install or update a shortcut.
///
/// Returns the installed path on success.
pub fn install_shortcut(
    provider: &Provider,
    profile_dir: &Path,
    target_dir: &Path,
) -> Result<PathBuf, AppError> {
    let name = resolve_shortcut_name(provider);

    validate_shortcut_name(&name)?;

    let path = shortcut_path(&name, target_dir);
    let slug = crate::config::derive_provider_slug(&provider.id);

    // If file exists, check ownership
    if path.exists() && !is_managed_shortcut(&path, &slug)? {
        return Err(AppError::Config(format!(
            "Cannot overwrite unmanaged command: {}",
            path.display()
        )));
    }

    // Ensure target directory exists
    fs::create_dir_all(target_dir).map_err(|e| AppError::io(target_dir, e))?;

    // Write wrapper script
    let script = render_wrapper_script(profile_dir, &provider.id);
    write_executable(&path, script.as_bytes())?;

    log::info!(
        "Installed shortcut: {} -> {}",
        path.display(),
        profile_dir.display()
    );
    Ok(path)
}

/// Remove a managed shortcut.
pub fn remove_shortcut(provider: &Provider, target_dir: &Path) -> Result<bool, AppError> {
    let name = resolve_shortcut_name(provider);
    remove_shortcut_by_name(provider, &name, target_dir)
}

/// Remove a managed shortcut by explicit command name.
pub fn remove_shortcut_by_name(
    provider: &Provider,
    name: &str,
    target_dir: &Path,
) -> Result<bool, AppError> {
    validate_shortcut_name(name)?;
    let path = shortcut_path(name, target_dir);
    if !path.exists() {
        return Ok(false);
    }

    let slug = crate::config::derive_provider_slug(&provider.id);
    if !is_managed_shortcut(&path, &slug)? {
        log::warn!("Refusing to remove unmanaged shortcut: {}", path.display());
        return Ok(false);
    }

    fs::remove_file(&path).map_err(|e| AppError::io(&path, e))?;
    log::info!("Removed shortcut: {}", path.display());
    Ok(true)
}

/// Resolve the profile directory for a provider.
fn resolve_profile_dir(provider: &Provider) -> PathBuf {
    let slug = crate::config::derive_provider_slug(&provider.id);
    let path_override = provider
        .meta
        .as_ref()
        .and_then(|m| m.managed_profile_path.as_ref())
        .map(PathBuf::from);

    let target = ClaudeConfigTarget::ManagedProfile {
        slug,
        path_override,
    };
    target.config_dir()
}

/// Extract the CLAUDE_CONFIG_DIR value from a wrapper script.
fn extract_profile_dir_from_script(content: &str) -> Option<String> {
    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("# Profile: ") {
            return Some(rest.to_string());
        }
    }

    for line in content.lines() {
        if let Some(rest) = line.trim().strip_prefix("export CLAUDE_CONFIG_DIR=") {
            // Remove surrounding quotes
            let unquoted = rest.trim_matches('"').trim_matches('\'');
            return Some(unquoted.to_string());
        }
    }
    None
}

/// Write content to a file and make it executable.
fn write_executable(path: &Path, content: &[u8]) -> Result<(), AppError> {
    let mut file = fs::File::create(path).map_err(|e| AppError::io(path, e))?;
    file.write_all(content).map_err(|e| AppError::io(path, e))?;
    file.flush().map_err(|e| AppError::io(path, e))?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = fs::Permissions::from_mode(0o755);
        fs::set_permissions(path, perms).map_err(|e| AppError::io(path, e))?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{ClaudeLauncherPermissionMode, Provider, ProviderMeta};
    use serde_json::json;
    use tempfile::tempdir;

    #[test]
    fn get_user_bin_dir_uses_local_bin() {
        let path = get_user_bin_dir();
        assert!(path.ends_with(".local/bin"));
    }

    #[test]
    fn generate_shortcut_name_uses_provider_display_name() {
        assert_eq!(generate_shortcut_name("Kimi"), "claude-kimi");
        assert_eq!(
            generate_shortcut_name("DeepSeek Chat"),
            "claude-deepseek-chat"
        );
    }

    #[test]
    fn default_shortcut_name_uses_provider_name_not_id() {
        let provider = Provider::with_id(
            "universal-claude-generated-id".to_string(),
            "Kimi".to_string(),
            json!({}),
            None,
        );

        assert_eq!(resolve_shortcut_name(&provider), "claude-kimi");
    }

    #[test]
    fn saved_shortcut_name_is_preserved() {
        let mut provider = Provider::with_id(
            "universal-claude-generated-id".to_string(),
            "Kimi".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-custom".to_string()),
            ..Default::default()
        });

        assert_eq!(resolve_shortcut_name(&provider), "claude-custom");
    }

    #[test]
    fn legacy_id_based_shortcut_name_migrates_to_provider_name_default() {
        let mut provider = Provider::with_id(
            "universal-claude-8f4b91d2".to_string(),
            "Kimi".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-8f4b91d2".to_string()),
            ..Default::default()
        });

        assert_eq!(resolve_shortcut_name(&provider), "claude-kimi");
    }

    #[test]
    fn validate_shortcut_name_rejects_empty() {
        assert!(validate_shortcut_name("").is_err());
    }

    #[test]
    fn validate_shortcut_name_rejects_path_separators() {
        assert!(validate_shortcut_name("claude/foo").is_err());
        assert!(validate_shortcut_name("claude\\bar").is_err());
    }

    #[test]
    fn validate_shortcut_name_rejects_shell_metacharacters() {
        assert!(validate_shortcut_name("claude test").is_err());
        assert!(validate_shortcut_name("claude;test").is_err());
        assert!(validate_shortcut_name("claude$test").is_err());
    }

    #[test]
    fn validate_shortcut_name_rejects_leading_hyphen() {
        assert!(validate_shortcut_name("-test").is_err());
    }

    #[test]
    fn validate_shortcut_name_rejects_dot_entries() {
        assert!(validate_shortcut_name(".").is_err());
        assert!(validate_shortcut_name("..").is_err());
    }

    #[test]
    fn validate_shortcut_name_accepts_valid() {
        assert!(validate_shortcut_name("claude-kimi").is_ok());
        assert!(validate_shortcut_name("claude_test").is_ok());
    }

    #[test]
    fn render_wrapper_contains_profile_dir_and_marker() {
        let profile = PathBuf::from("/tmp/test-profile");
        let script = render_wrapper_script(&profile, "universal-claude-kimi");

        assert!(script.contains("export CLAUDE_CONFIG_DIR='/tmp/test-profile'"));
        assert!(script.contains("cc-switch-launcher-settings.json"));
        assert!(script.contains("exec claude --settings \"$cc_switch_launcher_settings\" \"$@\""));
        assert!(script.contains("exec claude \"$@\""));
        assert!(script.contains("# cc-switch:managed:kimi"));
        assert!(!script.contains("API_KEY"));
        assert!(!script.contains("sk-"));
    }

    #[test]
    fn parse_ownership_marker_finds_managed_slug() {
        let script =
            "#!/usr/bin/env bash\n# cc-switch:managed:kimi\nexport CLAUDE_CONFIG_DIR='/tmp'\n";
        assert_eq!(parse_ownership_marker(script), Some("kimi".to_string()));
    }

    #[test]
    fn parse_ownership_marker_returns_none_for_unmanaged() {
        let script = "#!/usr/bin/env bash\necho hello\n";
        assert_eq!(parse_ownership_marker(script), None);
    }

    #[test]
    fn extract_profile_dir_finds_config_dir() {
        let script =
            "#!/usr/bin/env bash\nexport CLAUDE_CONFIG_DIR='/tmp/profile'\nexec claude \"$@\"\n";
        assert_eq!(
            extract_profile_dir_from_script(script),
            Some("/tmp/profile".to_string())
        );
    }

    #[test]
    fn extract_profile_dir_prefers_profile_comment() {
        let script = "#!/usr/bin/env bash\n# Profile: /tmp/profile O'Brien\nexport CLAUDE_CONFIG_DIR='/tmp/profile O'\"'\"'Brien'\nexec claude \"$@\"\n";
        assert_eq!(
            extract_profile_dir_from_script(script),
            Some("/tmp/profile O'Brien".to_string())
        );
    }

    #[test]
    fn install_and_remove_shortcut_roundtrip() {
        let temp = tempdir().expect("tempdir");
        let target_dir = temp.path().join("bin");

        let profile_dir = temp.path().join("profile");
        fs::create_dir_all(&profile_dir).unwrap();

        let mut provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-test".to_string()),
            managed_profile_path: Some(profile_dir.to_string_lossy().to_string()),
            ..Default::default()
        });

        // Install
        let installed_path =
            install_shortcut(&provider, &profile_dir, &target_dir).expect("install");
        assert!(installed_path.exists());
        assert!(installed_path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("claude-test"));

        // Check status
        let info = get_shortcut_status(&provider, &target_dir).expect("status");
        assert_eq!(info.status, ShortcutStatus::Installed);

        // Remove
        let removed = remove_shortcut(&provider, &target_dir).expect("remove");
        assert!(removed);
        assert!(!installed_path.exists());

        // Check status after removal
        let info = get_shortcut_status(&provider, &target_dir).expect("status");
        assert_eq!(info.status, ShortcutStatus::Missing);
    }

    #[test]
    fn remove_shortcut_by_name_removes_previous_alias() {
        let temp = tempdir().expect("tempdir");
        let target_dir = temp.path().join("bin");
        let profile_dir = temp.path().join("profile");
        fs::create_dir_all(&profile_dir).unwrap();

        let mut provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-old".to_string()),
            managed_profile_path: Some(profile_dir.to_string_lossy().to_string()),
            ..Default::default()
        });

        let old_path = install_shortcut(&provider, &profile_dir, &target_dir).expect("install");
        assert!(old_path.exists());

        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-new".to_string()),
            managed_profile_path: Some(profile_dir.to_string_lossy().to_string()),
            ..Default::default()
        });
        install_shortcut(&provider, &profile_dir, &target_dir).expect("install new");

        let removed =
            remove_shortcut_by_name(&provider, "claude-old", &target_dir).expect("remove old");
        assert!(removed);
        assert!(!old_path.exists());
        assert!(target_dir.join("claude-new").exists());
    }

    #[test]
    fn install_refuses_unmanaged_conflict() {
        let temp = tempdir().expect("tempdir");
        let target_dir = temp.path().join("bin");
        fs::create_dir_all(&target_dir).unwrap();

        // Write an unmanaged file
        let existing = target_dir.join("claude-test");
        fs::write(&existing, "#!/bin/bash\necho hello\n").unwrap();

        let provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({}),
            None,
        );

        let profile_dir = temp.path().join("profile");
        let result = install_shortcut(&provider, &profile_dir, &target_dir);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("unmanaged command"));
    }

    #[test]
    fn stale_detection_when_profile_path_changes() {
        let temp = tempdir().expect("tempdir");
        let target_dir = temp.path().join("bin");

        let mut provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-test".to_string()),
            ..Default::default()
        });

        let old_profile = temp.path().join("old-profile");
        fs::create_dir_all(&old_profile).unwrap();

        // Install with old profile
        install_shortcut(&provider, &old_profile, &target_dir).expect("install");

        // Change managed profile path to simulate a path change
        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-test".to_string()),
            managed_profile_path: Some("/new/profile/path".to_string()),
            ..Default::default()
        });

        // Status should detect stale
        let info = get_shortcut_status(&provider, &target_dir).expect("status");
        assert_eq!(info.status, ShortcutStatus::Stale);
    }

    #[test]
    fn old_wrapper_is_stale_when_permission_overlay_required() {
        let temp = tempdir().expect("tempdir");
        let target_dir = temp.path().join("bin");
        fs::create_dir_all(&target_dir).unwrap();
        let profile_dir = temp.path().join("profile");
        fs::create_dir_all(&profile_dir).unwrap();

        let mut provider = Provider::with_id(
            "universal-claude-test".to_string(),
            "Test".to_string(),
            json!({}),
            None,
        );
        provider.meta = Some(ProviderMeta {
            shortcut_name: Some("claude-test".to_string()),
            managed_profile_path: Some(profile_dir.to_string_lossy().to_string()),
            launcher_permission_mode: Some(ClaudeLauncherPermissionMode::Plan),
            ..Default::default()
        });

        let old_script = format!(
            "#!/usr/bin/env bash\n# cc-switch:managed:test\n# Profile: {}\nexport CLAUDE_CONFIG_DIR='{}'\nexec claude \"$@\"\n",
            profile_dir.display(),
            profile_dir.display()
        );
        fs::write(target_dir.join("claude-test"), old_script).unwrap();

        let info = get_shortcut_status(&provider, &target_dir).expect("status");

        assert_eq!(info.status, ShortcutStatus::Stale);
    }
}
