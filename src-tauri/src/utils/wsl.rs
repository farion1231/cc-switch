//! Shared WSL utilities used across multiple modules.
//!
//! Centralises the repeated helper code that each module previously copied:
//! - `decode_wsl_output`
//! - `parse_wsl_unc_path`
//! - `resolve_wsl_home_dir_unc`
//! - `is_valid_wsl_distro_name`
//! - `get_all_wsl_distros`
//! - `dedupe_paths`

use std::path::{Path, PathBuf};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

/// Suppress console window for spawned processes on Windows.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

// ─── Encoding helpers ──────────────────────────────────────────────────────

/// Decode bytes from `wsl.exe` output, which can be either UTF-8 or UTF-16LE.
///
/// `wsl.exe` on some Windows environments emits UTF-16LE (each ASCII character
/// is stored as a 2-byte pair with the high byte being `\0`).  Try UTF-8 first
/// and only fall back to UTF-16LE when NUL bytes are present.
#[cfg(target_os = "windows")]
pub fn decode_wsl_output(bytes: &[u8]) -> String {
    // UTF-8 that contains no embedded NULs is unambiguously correct.
    if let Ok(s) = String::from_utf8(bytes.to_vec()) {
        if !s.contains('\0') {
            return s;
        }
    }

    // Fall back to UTF-16LE.
    if bytes.len() >= 2 {
        let mut u16_buf = Vec::with_capacity(bytes.len() / 2);
        for chunk in bytes.chunks_exact(2) {
            u16_buf.push(u16::from_le_bytes([chunk[0], chunk[1]]));
        }
        if let Ok(s) = String::from_utf16(&u16_buf) {
            return s;
        }
    }

    String::from_utf8_lossy(bytes).into_owned()
}

// ─── Distro name validation ─────────────────────────────────────────────────

/// Returns `true` when `name` is a valid WSL distro identifier.
///
/// Allowed characters: ASCII alphanumeric, `-`, `_`, `.`; max length 64.
#[cfg(target_os = "windows")]
pub fn is_valid_wsl_distro_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

// ─── Distro enumeration ─────────────────────────────────────────────────────

/// Return all installed WSL distros, excluding docker-desktop entries and
/// entries with invalid names.  Returns an empty `Vec` when WSL is not
/// available.
#[cfg(target_os = "windows")]
pub fn get_all_wsl_distros() -> Vec<String> {
    use std::process::Command;

    let output = match Command::new("wsl.exe")
        .args(["--list", "--quiet"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
    {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };

    let text = decode_wsl_output(&output.stdout);
    text.lines()
        .map(|line| {
            line.trim()
                .trim_matches('\u{feff}') // BOM
                .trim_matches('\0')
                .replace('\0', "")
        })
        .filter(|line| !line.is_empty())
        .filter(|line| !line.starts_with('*'))
        .filter(|line| is_valid_wsl_distro_name(line))
        .filter(|line| !line.to_ascii_lowercase().contains("docker"))
        .collect()
}

// ─── UNC path helpers ───────────────────────────────────────────────────────

/// Parse a `\\wsl$\<distro>\…` or `\\wsl.localhost\<distro>\…` UNC path.
///
/// Returns `(distro_name, suffix)` where `suffix` is the path portion after
/// the distro component (may be empty).
#[cfg(target_os = "windows")]
pub fn parse_wsl_unc_path(path: &Path) -> Option<(String, String)> {
    let s = path.to_string_lossy();
    let s_lower = s.to_ascii_lowercase();
    for prefix in ["\\\\wsl$\\", "\\\\wsl.localhost\\"] {
        if s_lower.starts_with(prefix) {
            let rest = &s[prefix.len()..];
            let mut parts = rest.split('\\');
            let distro = parts.next()?.trim().to_string();
            if distro.is_empty() {
                return None;
            }
            let suffix = parts.collect::<Vec<_>>().join("\\");
            return Some((distro, suffix));
        }
    }
    None
}

/// Resolve a WSL distro's home directory as a Windows UNC (`\\wsl.localhost\…`)
/// path.  Returns `None` when the distro is unavailable or the query fails.
#[cfg(target_os = "windows")]
pub fn resolve_wsl_home_dir_unc(distro: &str) -> Option<PathBuf> {
    use std::process::Command;

    let distro = distro.trim();
    if distro.is_empty() {
        return None;
    }

    let output = Command::new("wsl.exe")
        .args(["-d", distro, "--", "sh", "-lc", "printf %s \"$HOME\""])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let home_raw = decode_wsl_output(&output.stdout)
        .replace('\0', "")
        .trim()
        .to_string();

    if home_raw.is_empty() || !home_raw.starts_with('/') {
        return None;
    }

    let mut unc = PathBuf::from(format!("\\\\wsl.localhost\\{distro}"));
    for segment in home_raw.trim_start_matches('/').split('/') {
        if !segment.is_empty() {
            unc.push(segment);
        }
    }
    Some(unc)
}

#[cfg(target_os = "windows")]
fn append_default_subdir(mut home_unc: PathBuf, default_subdir: &[&str]) -> PathBuf {
    for segment in default_subdir {
        home_unc.push(segment);
    }
    home_unc
}

#[cfg(target_os = "windows")]
fn expand_secondary_wsl_dirs_from_homes<F>(
    current_distro: Option<&str>,
    distros: &[String],
    default_subdir: &[&str],
    mut resolve_home_unc: F,
) -> Vec<PathBuf>
where
    F: FnMut(&str) -> Option<PathBuf>,
{
    let mut dirs = Vec::new();

    for distro in distros {
        let distro = distro.trim();
        if distro.is_empty()
            || current_distro.is_some_and(|current| distro.eq_ignore_ascii_case(current))
        {
            continue;
        }

        if let Some(home_unc) = resolve_home_unc(distro) {
            dirs.push(append_default_subdir(home_unc, default_subdir));
        }
    }

    dirs
}

// ─── Path utilities ─────────────────────────────────────────────────────────

/// Deduplicate a `Vec<PathBuf>`, preserving order.
///
/// On Windows the comparison is **case-insensitive** (Windows paths are
/// case-insensitive by default).
pub fn dedupe_paths(paths: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for path in paths {
        #[cfg(target_os = "windows")]
        let key = path.to_string_lossy().to_lowercase();
        #[cfg(not(target_os = "windows"))]
        let key = path.to_string_lossy().to_string();
        if seen.insert(key) {
            out.push(path);
        }
    }
    out
}

/// Expand a primary config directory into a set of directories that includes:
/// - the primary directory itself
/// - on Windows, additional WSL UNC directories inferred from known distros
///
/// This is used to support Windows + WSL dual-environment config sync.
pub(crate) fn expand_wsl_dirs(primary_dir: &Path, default_subdir: &[&str]) -> Vec<PathBuf> {
    #[cfg(target_os = "windows")]
    let mut dirs = vec![primary_dir.to_path_buf()];
    #[cfg(not(target_os = "windows"))]
    let dirs = vec![primary_dir.to_path_buf()];
    #[cfg(not(target_os = "windows"))]
    let _ = default_subdir;

    #[cfg(target_os = "windows")]
    {
        if let Some((current_distro, _suffix)) = parse_wsl_unc_path(primary_dir) {
            if let Some(distros) = crate::settings::get_wsl_distros() {
                dirs.extend(expand_secondary_wsl_dirs_from_homes(
                    Some(&current_distro),
                    &distros,
                    default_subdir,
                    resolve_wsl_home_dir_unc,
                ));
            }
        } else if let Some(distros) = crate::settings::get_wsl_distros() {
            dirs.extend(expand_secondary_wsl_dirs_from_homes(
                None,
                &distros,
                default_subdir,
                resolve_wsl_home_dir_unc,
            ));
        }
    }

    dedupe_paths(dirs)
}

// ─── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    #[cfg(target_os = "windows")]
    use super::*;
    #[cfg(target_os = "windows")]
    use std::collections::HashMap;
    #[cfg(target_os = "windows")]
    use std::path::{Path, PathBuf};

    #[cfg(target_os = "windows")]
    #[test]
    fn test_is_valid_wsl_distro_name() {
        assert!(is_valid_wsl_distro_name("Ubuntu"));
        assert!(is_valid_wsl_distro_name("Ubuntu-22.04"));
        assert!(is_valid_wsl_distro_name("my_distro"));
        assert!(!is_valid_wsl_distro_name(""));
        assert!(!is_valid_wsl_distro_name("distro with spaces"));
        assert!(!is_valid_wsl_distro_name(&"a".repeat(65)));
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_parse_wsl_unc_path_extracts_distro_and_suffix() {
        let parsed = parse_wsl_unc_path(Path::new(r"\\wsl.localhost\Ubuntu\home\alice\.codex"))
            .expect("should parse WSL UNC path");

        assert_eq!(parsed.0, "Ubuntu");
        assert_eq!(parsed.1, r"home\alice\.codex");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_parse_wsl_unc_path_accepts_case_insensitive_prefix() {
        let parsed = parse_wsl_unc_path(Path::new(r"\\WSL.LOCALHOST\Ubuntu\home\alice\.codex"))
            .expect("should parse WSL UNC path regardless of prefix case");

        assert_eq!(parsed.0, "Ubuntu");
        assert_eq!(parsed.1, r"home\alice\.codex");
    }

    #[cfg(target_os = "windows")]
    #[test]
    fn test_expand_secondary_wsl_dirs_from_homes_uses_each_distro_home() {
        let distros = vec![
            "Ubuntu".to_string(),
            "Debian".to_string(),
            "Arch".to_string(),
        ];
        let homes = HashMap::from([
            (
                "Debian".to_string(),
                PathBuf::from(r"\\wsl.localhost\Debian\home\bob"),
            ),
            (
                "Arch".to_string(),
                PathBuf::from(r"\\wsl.localhost\Arch\home\carol"),
            ),
        ]);

        let dirs =
            expand_secondary_wsl_dirs_from_homes(Some("Ubuntu"), &distros, &[".codex"], |distro| {
                homes.get(distro).cloned()
            });

        assert_eq!(
            dirs,
            vec![
                PathBuf::from(r"\\wsl.localhost\Debian\home\bob\.codex"),
                PathBuf::from(r"\\wsl.localhost\Arch\home\carol\.codex"),
            ]
        );
    }
}
