//! Tool Search domain restriction bypass patch for Claude Code.
//!
//! Resolves the current active `claude` command from PATH and patches the
//! domain whitelist check
//! `return["api.anthropic.com"].includes(x)}catch{return!1}`
//! to always return true via equal-length byte replacement.

use regex::bytes::Regex;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use crate::error::AppError;

const BACKUP_SUFFIX: &str = ".toolsearch-bak";

/// Encode bytes as lowercase hex string (avoids adding `hex` crate dependency).
fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// Regex matching the domain whitelist check with any JS identifier as variable name
const PATCH_TARGET_PATTERN: &str =
    r#"return\["api\.anthropic\.com"\]\.includes\([A-Za-z_$][A-Za-z0-9_$]*\)\}catch\{return!1\}"#;

// Regex matching already-patched code
const PATCHED_PATTERN: &str = r#"return!0/\* *\*/\}catch\{return!0\}"#;

/// Single Claude Code installation info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeInstallation {
    pub path: String,
    pub source: String,
    pub patched: bool,
    pub has_backup: bool,
}

/// Result of a patch/restore operation on one installation
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PatchResult {
    pub path: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Overall Tool Search patch status
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSearchStatus {
    pub installations: Vec<ClaudeInstallation>,
    pub all_patched: bool,
    pub any_found: bool,
}

// ── Patch status detection ──────────────────────────────────────────

fn get_patch_status(data: &[u8]) -> &'static str {
    let target_re = Regex::new(PATCH_TARGET_PATTERN).unwrap();
    let patched_re = Regex::new(PATCHED_PATTERN).unwrap();
    if target_re.is_match(data) {
        "unpatched"
    } else if patched_re.is_match(data) {
        "patched"
    } else {
        "unknown"
    }
}

/// Build equal-length replacement bytes: `return!0/*   */}catch{return!0}`
fn build_patched_bytes(original_len: usize) -> Result<Vec<u8>, AppError> {
    let prefix = b"return!0/*";
    let suffix = b"*/}catch{return!0}";
    let padding = original_len
        .checked_sub(prefix.len() + suffix.len())
        .ok_or_else(|| AppError::Config("Patch template too long for match".into()))?;
    let mut out = Vec::with_capacity(original_len);
    out.extend_from_slice(prefix);
    out.extend(std::iter::repeat_n(b' ', padding));
    out.extend_from_slice(suffix);
    Ok(out)
}

/// Apply byte-level patch to file data, returns (patched_data, replacement_count)
fn patch_bytes(data: &[u8]) -> Result<(Vec<u8>, usize), AppError> {
    let re = Regex::new(PATCH_TARGET_PATTERN).unwrap();
    let mut count = 0usize;
    let result = re.replace_all(data, |caps: &regex::bytes::Captures| {
        count += 1;
        build_patched_bytes(caps[0].len()).unwrap_or_else(|_| caps[0].to_vec())
    });
    Ok((result.into_owned(), count))
}

// ── Installation detection ──────────────────────────────────────────

/// Run a command and return stdout, or empty string on failure
fn run_cmd(cmd: &str, args: &[&str]) -> String {
    Command::new(cmd)
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
}

/// Search a package directory for JS files containing the domain check
fn find_patch_target_in_pkg(pkg_dir: &Path) -> Option<PathBuf> {
    let marker = b"api.anthropic.com";
    // Check cli.js first (most common)
    let cli_js = pkg_dir.join("cli.js");
    if cli_js.is_file() {
        if let Ok(data) = std::fs::read(&cli_js) {
            if data.windows(marker.len()).any(|w| w == marker) {
                return Some(cli_js);
            }
        }
    }
    // Search other JS files
    find_js_with_marker(pkg_dir)
}

fn find_js_with_marker(dir: &Path) -> Option<PathBuf> {
    let marker = b"api.anthropic.com";
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return None,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_js_with_marker(&path) {
                return Some(found);
            }
        } else if path.extension().and_then(|e| e.to_str()) == Some("js") {
            if let Ok(meta) = path.metadata() {
                if meta.len() < 1000 {
                    continue;
                }
            }
            if let Ok(data) = std::fs::read(&path) {
                if data.windows(marker.len()).any(|w| w == marker) {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Resolve symlinks to actual file path
fn resolve_target(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn get_patch_status_for_path(path: &Path) -> Option<&'static str> {
    let data = std::fs::read(path).ok()?;
    Some(get_patch_status(&data))
}

fn package_dir_from_ancestors(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|v| v.to_str()) == Some("claude-code")
            && ancestor.parent().and_then(|v| v.file_name()).and_then(|v| v.to_str())
                == Some("@anthropic-ai")
        {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn push_candidate_package_dir(
    candidates: &mut Vec<PathBuf>,
    seen: &mut std::collections::HashSet<PathBuf>,
    path: PathBuf,
) {
    if path.is_dir() && seen.insert(path.clone()) {
        candidates.push(path);
    }
}

fn resolve_active_patch_target(command_path: &Path) -> Option<PathBuf> {
    let resolved_command = resolve_target(command_path);
    if matches!(
        get_patch_status_for_path(&resolved_command),
        Some("patched" | "unpatched")
    ) {
        return Some(resolved_command);
    }

    let mut candidates = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for path in [command_path, resolved_command.as_path()] {
        if let Some(pkg_dir) = package_dir_from_ancestors(path) {
            push_candidate_package_dir(&mut candidates, &mut seen, pkg_dir);
        }

        if let Some(bin_dir) = path.parent() {
            if let Some(prefix) = bin_dir.parent() {
                push_candidate_package_dir(
                    &mut candidates,
                    &mut seen,
                    prefix
                        .join("lib")
                        .join("node_modules")
                        .join("@anthropic-ai")
                        .join("claude-code"),
                );
                push_candidate_package_dir(
                    &mut candidates,
                    &mut seen,
                    prefix
                        .join("node_modules")
                        .join("@anthropic-ai")
                        .join("claude-code"),
                );
            }
        }
    }

    candidates
        .into_iter()
        .find_map(|pkg_dir| find_patch_target_in_pkg(&pkg_dir).map(|p| resolve_target(&p)))
}

#[cfg(target_os = "windows")]
fn find_active_command_path() -> Option<PathBuf> {
    run_cmd("where.exe", &["claude"])
        .lines()
        .next()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

#[cfg(not(target_os = "windows"))]
fn find_active_command_path() -> Option<PathBuf> {
    run_cmd("which", &["claude"])
        .lines()
        .next()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
}

fn find_active_installation() -> Option<(PathBuf, String)> {
    let command_path = find_active_command_path()?;
    let patch_target = resolve_active_patch_target(&command_path)?;
    Some((
        patch_target,
        format!("active claude ({})", command_path.display()),
    ))
}

fn require_active_installation() -> Result<(PathBuf, String), AppError> {
    find_active_installation().ok_or_else(|| {
        AppError::Config("No active Claude Code installation found in PATH".into())
    })
}

// ── macOS codesign ──────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn codesign_adhoc(path: &Path) -> Result<(), AppError> {
    let output = Command::new("codesign")
        .args(["--force", "--sign", "-"])
        .arg(path)
        .output()
        .map_err(|e| AppError::IoContext {
            context: format!("Failed to run codesign for {}", path.display()),
            source: e,
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AppError::Config(format!(
            "codesign failed for {}: {}",
            path.display(),
            stderr.trim()
        )));
    }
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn codesign_adhoc(_path: &Path) -> Result<(), AppError> {
    Ok(())
}

// ── Backup directory helpers ─────────────────────────────────────────

/// Get the centralized backup directory: `~/.cc-switch/toolsearch-backups/`
fn get_backup_dir() -> Result<PathBuf, AppError> {
    let dir = crate::config::get_app_config_dir().join("toolsearch-backups");
    if !dir.exists() {
        std::fs::create_dir_all(&dir).map_err(|e| AppError::io(&dir, e))?;
    }
    Ok(dir)
}

/// Derive a stable backup filename from the original path using SHA-256.
fn backup_name_for(path: &Path) -> String {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    to_hex(&hasher.finalize())
}

/// Get the backup file path for a given target file.
fn get_backup_path(path: &Path) -> Result<PathBuf, AppError> {
    let dir = get_backup_dir()?;
    let name = backup_name_for(path);
    Ok(dir.join(format!("{name}.bak")))
}

/// Get the metadata file path (records original path for debugging).
fn get_meta_path(path: &Path) -> Result<PathBuf, AppError> {
    let dir = get_backup_dir()?;
    let name = backup_name_for(path);
    Ok(dir.join(format!("{name}.meta")))
}

// ── Patch / Restore single file ─────────────────────────────────────

fn patch_single_file(path: &Path) -> Result<(), AppError> {
    let data = std::fs::read(path).map_err(|e| AppError::io(path, e))?;
    let status = get_patch_status(&data);

    if status == "patched" {
        return Ok(()); // Already patched
    }
    if status == "unknown" {
        return Err(AppError::Config(format!(
            "Target pattern not found in {}, possibly incompatible version",
            path.display()
        )));
    }

    let (patched_data, count) = patch_bytes(&data)?;
    if count == 0 {
        return Err(AppError::Config(format!(
            "No replacements made in {}",
            path.display()
        )));
    }

    // Create backup in centralized directory
    let backup_path = get_backup_path(path)?;
    std::fs::copy(path, &backup_path).map_err(|e| AppError::io(&backup_path, e))?;

    // Write metadata file for debugging
    let meta_path = get_meta_path(path)?;
    let _ = std::fs::write(&meta_path, path.to_string_lossy().as_bytes());

    // Write patched data
    if let Err(e) = std::fs::write(path, &patched_data) {
        // Try rename trick on Windows
        #[cfg(target_os = "windows")]
        {
            if let Ok(()) = write_via_rename(path, &patched_data) {
                codesign_adhoc(path)?;
                return Ok(());
            }
        }
        return Err(AppError::io(path, e));
    }

    codesign_adhoc(path)?;
    Ok(())
}

#[cfg(target_os = "windows")]
fn write_via_rename(target: &Path, data: &[u8]) -> Result<(), AppError> {
    let tmp_path = target.with_extension("tmp");
    let old_path = target.with_extension("old");

    let _ = std::fs::remove_file(&tmp_path);
    let _ = std::fs::remove_file(&old_path);

    std::fs::write(&tmp_path, data).map_err(|e| AppError::io(&tmp_path, e))?;
    std::fs::rename(target, &old_path).map_err(|e| AppError::io(target, e))?;
    std::fs::rename(&tmp_path, target).map_err(|e| AppError::io(target, e))?;
    let _ = std::fs::remove_file(&old_path);
    Ok(())
}

fn restore_single_file(path: &Path) -> Result<(), AppError> {
    let current_status = get_patch_status_for_path(path);
    if matches!(current_status, Some("unpatched")) {
        return Ok(());
    }

    // Try centralized backup directory first
    let backup_path = get_backup_path(path)?;
    // Fallback: legacy backup path (adjacent `.toolsearch-bak` file)
    let legacy_backup = PathBuf::from(format!("{}{}", path.display(), BACKUP_SUFFIX));

    let actual_backup = if backup_path.is_file() {
        &backup_path
    } else if legacy_backup.is_file() {
        &legacy_backup
    } else {
        return Err(AppError::Config(format!(
            "No backup found for {}",
            path.display()
        )));
    };

    let backup_data = std::fs::read(actual_backup).map_err(|e| AppError::io(actual_backup, e))?;

    if let Err(e) = std::fs::write(path, &backup_data) {
        #[cfg(target_os = "windows")]
        {
            if let Ok(()) = write_via_rename(path, &backup_data) {
                codesign_adhoc(path)?;
                return Ok(());
            }
        }
        return Err(AppError::io(path, e));
    }

    codesign_adhoc(path)?;
    Ok(())
}

// ── Public API ──────────────────────────────────────────────────────

/// Check Tool Search patch status for the current active Claude Code installation.
pub fn check_toolsearch_status() -> Result<ToolSearchStatus, AppError> {
    let installations = find_active_installation()
        .into_iter()
        .map(|(path, source)| {
            let data = std::fs::read(&path).unwrap_or_default();
            let status = get_patch_status(&data);
            // Check centralized backup first, then legacy
            let has_backup = get_backup_path(&path)
                .map(|p| p.is_file())
                .unwrap_or(false)
                || PathBuf::from(format!("{}{}", path.display(), BACKUP_SUFFIX)).is_file();
            ClaudeInstallation {
                path: path.to_string_lossy().to_string(),
                source: source.clone(),
                patched: status == "patched",
                has_backup,
            }
        })
        .collect::<Vec<_>>();

    let any_found = !installations.is_empty();
    let all_patched = any_found && installations.iter().all(|i| i.patched);

    Ok(ToolSearchStatus {
        installations,
        all_patched,
        any_found,
    })
}

/// Apply the Tool Search patch to the current active Claude Code installation.
pub fn apply_toolsearch_patch() -> Result<Vec<PatchResult>, AppError> {
    let (path, _) = require_active_installation()?;
    Ok(vec![match patch_single_file(&path) {
        Ok(()) => PatchResult {
            path: path.to_string_lossy().to_string(),
            success: true,
            error: None,
        },
        Err(e) => PatchResult {
            path: path.to_string_lossy().to_string(),
            success: false,
            error: Some(e.to_string()),
        },
    }])
}

/// Restore the current active Claude Code installation from backup.
pub fn restore_toolsearch_patch() -> Result<Vec<PatchResult>, AppError> {
    let (path, _) = require_active_installation()?;
    Ok(vec![match restore_single_file(&path) {
        Ok(()) => PatchResult {
            path: path.to_string_lossy().to_string(),
            success: true,
            error: None,
        },
        Err(e) => PatchResult {
            path: path.to_string_lossy().to_string(),
            success: false,
            error: Some(e.to_string()),
        },
    }])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_patch_bytes_replaces_correctly() {
        let input = br#"return["api.anthropic.com"].includes(x)}catch{return!1}"#;
        let (patched, count) = patch_bytes(input).unwrap();
        assert_eq!(count, 1);
        assert_eq!(patched.len(), input.len());
        assert!(patched.starts_with(b"return!0/*"));
        assert!(patched.ends_with(b"*/}catch{return!0}"));
    }

    #[test]
    fn test_patch_status_detection() {
        let unpatched = br#"return["api.anthropic.com"].includes(x)}catch{return!1}"#;
        assert_eq!(get_patch_status(unpatched), "unpatched");

        let (patched, _) = patch_bytes(unpatched).unwrap();
        assert_eq!(get_patch_status(&patched), "patched");

        assert_eq!(get_patch_status(b"some random data"), "unknown");
    }

    #[test]
    fn test_build_patched_bytes_length() {
        for len in 50..70 {
            let result = build_patched_bytes(len).unwrap();
            assert_eq!(result.len(), len);
        }
    }

    #[test]
    fn test_resolve_active_patch_target_from_npm_style_bin_path() {
        let tmp = tempfile::tempdir().unwrap();
        let prefix = tmp.path().join("prefix");
        let bin_dir = prefix.join("bin");
        let pkg_dir = prefix
            .join("lib")
            .join("node_modules")
            .join("@anthropic-ai")
            .join("claude-code");
        std::fs::create_dir_all(&bin_dir).unwrap();
        std::fs::create_dir_all(&pkg_dir).unwrap();

        let command_path = bin_dir.join("claude");
        std::fs::write(&command_path, b"#!/usr/bin/env node\n").unwrap();
        let cli_path = pkg_dir.join("cli.js");
        std::fs::write(
            &cli_path,
            br#"return["api.anthropic.com"].includes(x)}catch{return!1}"#,
        )
        .unwrap();

        assert_eq!(
            resolve_active_patch_target(&command_path),
            Some(resolve_target(&cli_path))
        );
    }

    #[test]
    fn test_restore_single_file_is_noop_when_current_target_is_unpatched() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(
            tmp.path(),
            br#"return["api.anthropic.com"].includes(x)}catch{return!1}"#,
        )
        .unwrap();

        assert!(restore_single_file(tmp.path()).is_ok());
    }
}
