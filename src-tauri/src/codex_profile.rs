//! Managed Codex account profiles.
//!
//! `CODEX_HOME` is account-specific so every official ChatGPT account owns a
//! separate `auth.json`. Everything else continues to live in the user's normal
//! Codex directory and is projected into the account profile.

use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{atomic_write, get_app_config_dir, get_home_dir};
use crate::error::AppError;

const CODEX_HOME_ENV: &str = "CODEX_HOME";
const CODEX_SQLITE_HOME_ENV: &str = "CODEX_SQLITE_HOME";
const PROFILE_DIR_NAME: &str = "codex-profiles";
const ENV_FILE_NAME: &str = "codex-env.sh";
const SHELL_SOURCE_MARKER: &str = "# CC Switch managed Codex profile";

/// The canonical, shared Codex directory. This deliberately ignores
/// `CODEX_HOME`, which may already point at an account profile.
pub fn shared_codex_dir() -> PathBuf {
    crate::settings::get_codex_override_dir().unwrap_or_else(|| get_home_dir().join(".codex"))
}

pub fn managed_profiles_dir() -> PathBuf {
    get_app_config_dir().join(PROFILE_DIR_NAME)
}

pub fn account_profile_dir(account_id: &str) -> PathBuf {
    let digest = Sha256::digest(account_id.as_bytes());
    managed_profiles_dir().join(format!("account-{:x}", digest))
}

pub fn active_codex_dir_for_account(account_id: Option<&str>) -> PathBuf {
    account_id
        .map(account_profile_dir)
        .unwrap_or_else(shared_codex_dir)
}

/// Prepare an account profile. `auth.json` and `config.toml` are intentionally
/// excluded from links: auth is private, while config is copied on activation
/// because Codex/CC Switch replace it atomically (which would sever a symlink).
pub fn prepare_account_profile(account_id: &str) -> Result<PathBuf, AppError> {
    let shared = shared_codex_dir();
    let profile = account_profile_dir(account_id);
    fs::create_dir_all(&shared).map_err(|e| AppError::io(&shared, e))?;
    fs::create_dir_all(&profile).map_err(|e| AppError::io(&profile, e))?;

    for required in ["sessions", "archived_sessions", "skills"] {
        let path = shared.join(required);
        fs::create_dir_all(&path).map_err(|e| AppError::io(&path, e))?;
    }
    for required in ["history.jsonl", "session_index.jsonl"] {
        let path = shared.join(required);
        if !path.exists() {
            atomic_write(&path, b"")?;
        }
        ensure_shared_link(&path, &profile.join(required))?;
    }

    for entry in fs::read_dir(&shared).map_err(|e| AppError::io(&shared, e))? {
        let entry = entry.map_err(|e| AppError::io(&shared, e))?;
        let name = entry.file_name();
        if name == "auth.json"
            || name == "config.toml"
            || name == "accounts"
            || name == "mcp-oauth-locks"
            || !entry.path().is_dir()
        {
            continue;
        }
        let source = entry.path();
        let target = profile.join(&name);
        ensure_shared_link(&source, &target)?;
    }

    let shared_config = shared.join("config.toml");
    let profile_config = profile.join("config.toml");
    if shared_config.is_file() {
        fs::copy(&shared_config, &profile_config).map_err(|e| AppError::io(&profile_config, e))?;
    }

    Ok(profile)
}

pub fn activate(account_id: Option<&str>) -> Result<PathBuf, AppError> {
    let shared = shared_codex_dir();
    fs::create_dir_all(&shared).map_err(|e| AppError::io(&shared, e))?;
    mirror_active_config_to_shared()?;
    let active = match account_id {
        Some(account_id) => prepare_account_profile(account_id)?,
        None => shared.clone(),
    };

    persist_user_environment(&active, &shared)?;
    // SAFETY: CC Switch owns these two process variables and serializes Codex
    // provider switches. Child Codex processes inherit the newly active profile.
    std::env::set_var(CODEX_HOME_ENV, &active);
    std::env::set_var(CODEX_SQLITE_HOME_ENV, &shared);
    Ok(active)
}

/// Keep `config.toml` shared without relying on a file symlink that Codex's
/// atomic replace would sever. This is called before changing profiles and
/// after every CC Switch live write.
pub fn mirror_active_config_to_shared() -> Result<(), AppError> {
    let Some(active) = std::env::var_os(CODEX_HOME_ENV).map(PathBuf::from) else {
        return Ok(());
    };
    let shared = shared_codex_dir();
    if active == shared || !active.starts_with(managed_profiles_dir()) {
        return Ok(());
    }
    let source = active.join("config.toml");
    if source.is_file() {
        let target = shared.join("config.toml");
        fs::copy(&source, &target).map_err(|e| AppError::io(&target, e))?;
    }
    Ok(())
}

pub fn write_account_auth(account_id: &str, auth: &serde_json::Value) -> Result<(), AppError> {
    let profile = prepare_account_profile(account_id)?;
    crate::config::write_private_json_file(&profile.join("auth.json"), auth)
}

pub fn read_account_auth(account_id: &str) -> Result<Option<serde_json::Value>, AppError> {
    let path = account_profile_dir(account_id).join("auth.json");
    if !path.is_file() {
        return Ok(None);
    }
    crate::config::read_json_file(&path).map(Some)
}

#[cfg(unix)]
fn ensure_shared_link(source: &Path, target: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::symlink;
    if target.exists() || target.symlink_metadata().is_ok() {
        return Ok(());
    }
    symlink(source, target).map_err(|e| AppError::io(target, e))
}

#[cfg(windows)]
fn ensure_shared_link(source: &Path, target: &Path) -> Result<(), AppError> {
    if target.exists() || target.symlink_metadata().is_ok() {
        return Ok(());
    }
    if source.is_dir() {
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(target)
            .arg(source)
            .status()
            .map_err(|e| AppError::io(target, e))?;
        if status.success() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "Failed to create Codex shared directory junction: {}",
                target.display()
            )))
        }
    } else {
        fs::hard_link(source, target).map_err(|e| AppError::io(target, e))
    }
}

#[cfg(not(target_os = "windows"))]
fn persist_user_environment(active: &Path, shared: &Path) -> Result<(), AppError> {
    let env_file = get_app_config_dir().join(ENV_FILE_NAME);
    let contents = render_posix_env_file(active, shared);
    atomic_write(&env_file, contents.as_bytes())?;

    let shell = std::env::var("SHELL").unwrap_or_default();
    let rc_path = if shell.ends_with("/zsh") {
        get_home_dir().join(".zshrc")
    } else if shell.ends_with("/bash") {
        get_home_dir().join(".bashrc")
    } else {
        get_home_dir().join(".profile")
    };
    ensure_shell_sources_env_file(&rc_path, &env_file)
}

#[cfg(target_os = "windows")]
fn persist_user_environment(active: &Path, shared: &Path) -> Result<(), AppError> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let (environment, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey("Environment")
        .map_err(|e| AppError::Message(format!("Failed to open HKCU\\Environment: {e}")))?;
    environment
        .set_value(CODEX_HOME_ENV, &active.to_string_lossy().as_ref())
        .map_err(|e| AppError::Message(format!("Failed to persist CODEX_HOME: {e}")))?;
    environment
        .set_value(CODEX_SQLITE_HOME_ENV, &shared.to_string_lossy().as_ref())
        .map_err(|e| AppError::Message(format!("Failed to persist CODEX_SQLITE_HOME: {e}")))?;
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn render_posix_env_file(active: &Path, shared: &Path) -> String {
    format!(
        "{SHELL_SOURCE_MARKER}\nexport {CODEX_HOME_ENV}={}\nexport {CODEX_SQLITE_HOME_ENV}={}\n",
        shell_quote(&active.to_string_lossy()),
        shell_quote(&shared.to_string_lossy())
    )
}

#[cfg(not(target_os = "windows"))]
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(not(target_os = "windows"))]
fn ensure_shell_sources_env_file(rc_path: &Path, env_file: &Path) -> Result<(), AppError> {
    let source_line = format!(
        ". {} {SHELL_SOURCE_MARKER}",
        shell_quote(&env_file.to_string_lossy())
    );
    let current = match fs::read_to_string(rc_path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(AppError::io(rc_path, error)),
    };
    if current
        .lines()
        .any(|line| line.contains(SHELL_SOURCE_MARKER))
    {
        return Ok(());
    }
    let mut next = current;
    if !next.is_empty() && !next.ends_with('\n') {
        next.push('\n');
    }
    next.push_str(&source_line);
    next.push('\n');
    atomic_write(rc_path, next.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_profile_names_are_stable_and_path_safe() {
        let a = account_profile_dir("acct/with:unsafe chars");
        let b = account_profile_dir("acct/with:unsafe chars");
        assert_eq!(a, b);
        let name = a.file_name().unwrap().to_string_lossy();
        assert!(name.starts_with("account-"));
        assert!(name.chars().all(|ch| ch.is_ascii_hexdigit() || ch == '-'));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn posix_env_file_quotes_paths() {
        let rendered = render_posix_env_file(
            Path::new("/tmp/profile with ' quote"),
            Path::new("/tmp/shared"),
        );
        assert!(rendered.contains("export CODEX_HOME='/tmp/profile with '\\'' quote'"));
        assert!(rendered.contains("export CODEX_SQLITE_HOME='/tmp/shared'"));
    }

    #[cfg(not(target_os = "windows"))]
    #[test]
    fn shell_source_line_is_idempotent() {
        let temp = tempfile::tempdir().unwrap();
        let rc = temp.path().join(".zshrc");
        let env_file = temp.path().join("codex-env.sh");
        ensure_shell_sources_env_file(&rc, &env_file).unwrap();
        ensure_shell_sources_env_file(&rc, &env_file).unwrap();
        let text = fs::read_to_string(rc).unwrap();
        assert_eq!(text.matches(SHELL_SOURCE_MARKER).count(), 1);
    }
}
