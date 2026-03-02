//! Per-provider instance directory management
//!
//! Each Claude provider gets an isolated config directory under
//! `~/.cc-switch/instances/<provider_id>/`, populated with a `settings.json`
//! and symlinks to shared entries in `~/.claude/`.
//!
//! This enables running `CLAUDE_CONFIG_DIR=<instance_dir> claude` for any
//! provider without touching the shared live config.

use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::{get_app_config_dir, get_claude_config_dir};
use crate::error::AppError;

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Returns `~/.cc-switch/instances`
pub fn get_instances_root() -> PathBuf {
    get_app_config_dir().join("instances")
}


// ---------------------------------------------------------------------------
// Public API (uses real paths)
// ---------------------------------------------------------------------------

/// Create (or refresh) the instance directory for `provider_id`.
///
/// Writes `settings.json` and creates symlinks for every entry in `~/.claude/`
/// except `settings.json`.
pub fn ensure_instance_dir(provider_id: &str, settings: &Value) -> Result<(), AppError> {
    ensure_instance_dir_with_paths(
        provider_id,
        settings,
        &get_claude_config_dir(),
        &get_instances_root(),
    )
}

/// Update `settings.json` inside an existing instance directory.
pub fn sync_instance_settings(provider_id: &str, settings: &Value) -> Result<(), AppError> {
    sync_instance_settings_with_paths(
        provider_id,
        settings,
        &get_claude_config_dir(),
        &get_instances_root(),
    )
}

/// Delete the instance directory for `provider_id`.
pub fn remove_instance_dir(provider_id: &str) -> Result<(), AppError> {
    remove_instance_dir_with_paths(provider_id, &get_instances_root())
}

/// Batch-sync settings for providers that already have an instance directory.
///
/// Providers without an existing directory are silently skipped.
pub fn sync_all_instances(provider_settings: &[(&str, Value)]) -> Result<(), AppError> {
    let instances_root = get_instances_root();
    if !instances_root.exists() {
        return Ok(());
    }
    let claude_dir = get_claude_config_dir();
    for (provider_id, settings) in provider_settings {
        let instance_dir = instances_root.join(provider_id);
        if instance_dir.exists() {
            sync_instance_settings_with_paths(
                provider_id,
                settings,
                &claude_dir,
                &instances_root,
            )?;
        }
    }
    Ok(())
}

/// Generate a shell aliases script for the given providers.
pub fn export_aliases(providers: &[(String, String)]) -> String {
    let instances_root = get_instances_root();
    build_aliases_script(providers, &instances_root)
}

/// Strip internal-only fields before writing to an instance `settings.json`.
pub fn sanitize_for_instance(settings: &Value) -> Value {
    super::live::sanitize_claude_settings_for_live(settings)
}

// ---------------------------------------------------------------------------
// Testable internals (pub(crate) so inline tests can call them with temp dirs)
// ---------------------------------------------------------------------------

/// Create (or refresh) the instance directory at `<instance_root>/<provider_id>`.
pub(crate) fn ensure_instance_dir_with_paths(
    provider_id: &str,
    settings: &Value,
    claude_dir: &Path,
    instance_root: &Path,
) -> Result<(), AppError> {
    let instance_dir = instance_root.join(provider_id);

    // Create the instance directory tree
    fs::create_dir_all(&instance_dir).map_err(|e| AppError::io(&instance_dir, e))?;

    // Sanitize before writing to strip internal-only fields (api_format, etc.)
    let sanitized = super::live::sanitize_claude_settings_for_live(settings);

    // Write settings.json (direct overwrite, not atomic)
    let settings_path = instance_dir.join("settings.json");
    let json = serde_json::to_string_pretty(&sanitized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&settings_path, json).map_err(|e| AppError::io(&settings_path, e))?;

    // Symlink every entry in claude_dir except settings.json
    if claude_dir.exists() {
        let entries = fs::read_dir(claude_dir).map_err(|e| AppError::io(claude_dir, e))?;
        for entry in entries.flatten() {
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();
            if name == "settings.json" {
                continue;
            }

            let link_path = instance_dir.join(&*name);
            // Skip if already exists (file or broken symlink)
            if link_path.exists() || link_path.is_symlink() {
                continue;
            }

            let target = entry.path();
            if let Err(e) = create_symlink(&target, &link_path) {
                log::warn!(
                    "instance: failed to symlink {} -> {}: {e}",
                    link_path.display(),
                    target.display()
                );
            }
        }
    }

    // Also symlink ~/.claude.json (auth file that lives next to the config dir)
    if let Some(home_dir) = claude_dir.parent() {
        let dot_claude_json = home_dir.join(".claude.json");
        if dot_claude_json.exists() {
            let link_path = instance_dir.join(".claude.json");
            if !link_path.exists() && !link_path.is_symlink() {
                if let Err(e) = create_symlink(&dot_claude_json, &link_path) {
                    log::warn!(
                        "instance: failed to symlink {} -> {}: {e}",
                        link_path.display(),
                        dot_claude_json.display()
                    );
                }
            }
        }
    }

    Ok(())
}

/// Update `settings.json` inside `<instance_root>/<provider_id>`.
///
/// Falls back to a full `ensure_instance_dir_with_paths` if the directory
/// does not yet exist. `claude_dir` is used only in that fallback case.
pub(crate) fn sync_instance_settings_with_paths(
    provider_id: &str,
    settings: &Value,
    claude_dir: &Path,
    instance_root: &Path,
) -> Result<(), AppError> {
    let instance_dir = instance_root.join(provider_id);
    if !instance_dir.exists() {
        return ensure_instance_dir_with_paths(provider_id, settings, claude_dir, instance_root);
    }

    // Sanitize before writing (same as ensure_instance_dir_with_paths)
    let sanitized = super::live::sanitize_claude_settings_for_live(settings);
    let settings_path = instance_dir.join("settings.json");
    let json = serde_json::to_string_pretty(&sanitized)
        .map_err(|e| AppError::JsonSerialize { source: e })?;
    fs::write(&settings_path, json).map_err(|e| AppError::io(&settings_path, e))?;
    Ok(())
}

/// Remove `<instance_root>/<provider_id>` entirely. Silently succeeds if absent.
pub(crate) fn remove_instance_dir_with_paths(
    provider_id: &str,
    instance_root: &Path,
) -> Result<(), AppError> {
    let instance_dir = instance_root.join(provider_id);
    match fs::remove_dir_all(&instance_dir) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(AppError::io(&instance_dir, e)),
    }
}

/// Build a shell snippet that defines `claude-<alias>` aliases.
///
/// The script is wrapped in marker comments so it can be inserted into / removed
/// from a shell RC file predictably.
pub(crate) fn build_aliases_script(
    providers: &[(String, String)],
    instance_root: &Path,
) -> String {
    let date = today_str();
    let mut lines = Vec::new();
    lines.push(format!(
        "# --- cc-switch aliases begin (generated {date}) ---"
    ));
    for (provider_id, provider_name) in providers {
        let alias_name = provider_name_to_alias(provider_name);
        let dir = instance_root.join(provider_id);
        // Single-quote the path to handle spaces in home directory
        lines.push(format!(
            "alias claude-{alias_name}='CLAUDE_CONFIG_DIR=\"{}\" claude'",
            dir.display()
        ));
    }
    lines.push("# --- cc-switch aliases end ---".to_string());
    lines.join("\n")
}

/// Convert a provider display name to a shell-alias-friendly string.
///
/// Rules:
/// - Lowercase
/// - Replace every non-alphanumeric character with `-`
/// - Collapse consecutive `-` into one
/// - Strip leading/trailing `-`
pub(crate) fn provider_name_to_alias(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut alias = String::with_capacity(lower.len());
    let mut prev_hyphen = false;
    for c in lower.chars() {
        if c.is_alphanumeric() {
            alias.push(c);
            prev_hyphen = false;
        } else {
            if !prev_hyphen {
                alias.push('-');
            }
            prev_hyphen = true;
        }
    }
    // Trim leading/trailing hyphens
    alias.trim_matches('-').to_string()
}

// ---------------------------------------------------------------------------
// Platform symlink helper
// ---------------------------------------------------------------------------

#[cfg(unix)]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, link)
}

#[cfg(windows)]
fn create_symlink(target: &Path, link: &Path) -> std::io::Result<()> {
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, link)
    } else {
        std::os::windows::fs::symlink_file(target, link)
    }
}

// ---------------------------------------------------------------------------
// Date calculation (no chrono dependency)
// ---------------------------------------------------------------------------

fn today_str() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let days = secs / 86400;
    let (y, m, d) = days_to_ymd(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn days_to_ymd(days: u64) -> (u64, u64, u64) {
    let mut year = 1970u64;
    let mut remaining = days;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let months = [
        31u64,
        if is_leap(year) { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 1u64;
    for days_in_month in months {
        if remaining < days_in_month {
            break;
        }
        remaining -= days_in_month;
        month += 1;
    }
    (year, month, remaining + 1)
}

fn is_leap(year: u64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_dirs() -> (TempDir, TempDir) {
        let claude_dir = TempDir::new().unwrap();
        let cc_switch_dir = TempDir::new().unwrap();
        (claude_dir, cc_switch_dir)
    }

    #[test]
    fn test_ensure_instance_dir_creates_settings_json() {
        let (claude_dir, cc_switch_dir) = setup_dirs();
        let instance_root = cc_switch_dir.path().join("instances");

        ensure_instance_dir_with_paths(
            "provider-abc",
            &serde_json::json!({"apiKey": "sk-test"}),
            &claude_dir.path(),
            &instance_root,
        )
        .unwrap();

        let settings_path = instance_root.join("provider-abc").join("settings.json");
        assert!(settings_path.exists());
        let content: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(settings_path).unwrap()).unwrap();
        assert_eq!(content["apiKey"], "sk-test");
    }

    #[test]
    fn test_ensure_instance_dir_creates_symlinks() {
        let (claude_dir, cc_switch_dir) = setup_dirs();
        // Create some dirs/files in claude_dir to symlink
        fs::create_dir(claude_dir.path().join("plugins")).unwrap();
        fs::create_dir(claude_dir.path().join("commands")).unwrap();
        let instance_root = cc_switch_dir.path().join("instances");

        ensure_instance_dir_with_paths(
            "provider-abc",
            &serde_json::json!({}),
            &claude_dir.path(),
            &instance_root,
        )
        .unwrap();

        let plugins_link = instance_root.join("provider-abc").join("plugins");
        assert!(plugins_link.exists());
        assert!(plugins_link.is_symlink());
    }

    #[test]
    fn test_ensure_instance_dir_symlinks_dot_claude_json() {
        let (claude_dir, cc_switch_dir) = setup_dirs();
        // Create ~/.claude.json next to the claude_dir (i.e., in its parent)
        let home_dir = claude_dir.path().parent().unwrap();
        let dot_claude_json = home_dir.join(".claude.json");
        fs::write(&dot_claude_json, r#"{"token":"abc"}"#).unwrap();
        let instance_root = cc_switch_dir.path().join("instances");

        ensure_instance_dir_with_paths(
            "provider-abc",
            &serde_json::json!({}),
            claude_dir.path(),
            &instance_root,
        )
        .unwrap();

        let link = instance_root.join("provider-abc").join(".claude.json");
        assert!(link.exists(), ".claude.json symlink should exist in instance dir");
        assert!(link.is_symlink(), ".claude.json should be a symlink");
    }

    #[test]
    fn test_ensure_instance_dir_skips_missing_claude_entries() {
        let (claude_dir, cc_switch_dir) = setup_dirs();
        // claude_dir is empty — no plugins/, no commands/
        let instance_root = cc_switch_dir.path().join("instances");

        // Should not fail even though there's nothing to symlink
        ensure_instance_dir_with_paths(
            "provider-abc",
            &serde_json::json!({}),
            &claude_dir.path(),
            &instance_root,
        )
        .unwrap();

        let instance_dir = instance_root.join("provider-abc");
        assert!(instance_dir.exists());
    }

    #[test]
    fn test_sync_instance_settings_updates_settings_json() {
        let (claude_dir, cc_switch_dir) = setup_dirs();
        let instance_root = cc_switch_dir.path().join("instances");

        ensure_instance_dir_with_paths(
            "provider-abc",
            &serde_json::json!({"apiKey": "old"}),
            claude_dir.path(),
            &instance_root,
        )
        .unwrap();

        sync_instance_settings_with_paths(
            "provider-abc",
            &serde_json::json!({"apiKey": "new"}),
            claude_dir.path(),
            &instance_root,
        )
        .unwrap();

        let content: serde_json::Value = serde_json::from_str(
            &fs::read_to_string(instance_root.join("provider-abc").join("settings.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(content["apiKey"], "new");
    }

    #[test]
    fn test_remove_instance_dir_deletes_directory() {
        let (claude_dir, cc_switch_dir) = setup_dirs();
        let instance_root = cc_switch_dir.path().join("instances");

        ensure_instance_dir_with_paths(
            "provider-abc",
            &serde_json::json!({}),
            claude_dir.path(),
            &instance_root,
        )
        .unwrap();

        assert!(instance_root.join("provider-abc").exists());
        remove_instance_dir_with_paths("provider-abc", &instance_root).unwrap();
        assert!(!instance_root.join("provider-abc").exists());
    }

    #[test]
    fn test_remove_instance_dir_nonexistent_is_ok() {
        let cc_switch_dir = TempDir::new().unwrap();
        let instance_root = cc_switch_dir.path().join("instances");
        // Should silently succeed
        remove_instance_dir_with_paths("nonexistent", &instance_root).unwrap();
    }

    #[test]
    fn test_build_aliases_script() {
        let providers = vec![
            ("provider-abc".to_string(), "OpenRouter".to_string()),
            ("provider-def".to_string(), "Anthropic Official".to_string()),
        ];
        let instance_root = std::path::PathBuf::from("/home/user/.cc-switch/instances");
        let script = build_aliases_script(&providers, &instance_root);

        assert!(script.contains("alias claude-openrouter="));
        assert!(script.contains("alias claude-anthropic-official="));
        assert!(script.contains("CLAUDE_CONFIG_DIR=\""));
        assert!(script.contains("cc-switch aliases begin"));
        assert!(script.contains("cc-switch aliases end"));
    }
}
