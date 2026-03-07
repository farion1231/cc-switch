//! Configuration utilities

use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::AppError;

/// Get user home directory with fallback
pub fn get_home_dir() -> PathBuf {
    if let Ok(home) = std::env::var("CC_SWITCH_TEST_HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    dirs::home_dir().unwrap_or_else(|| {
        log::warn!("Cannot get user home directory, falling back to current directory");
        PathBuf::from(".")
    })
}

/// Get CC-Switch config directory (~/.cc-switch)
pub fn config_dir() -> PathBuf {
    get_home_dir().join(".cc-switch")
}

pub fn get_app_config_dir() -> PathBuf {
    config_dir()
}

/// Get database path
pub fn database_path() -> PathBuf {
    config_dir().join("cc-switch.db")
}

/// Get settings file path
pub fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

/// Get Claude Code config directory
pub fn get_claude_config_dir() -> PathBuf {
    crate::settings::get_claude_override_dir().unwrap_or_else(|| get_home_dir().join(".claude"))
}

/// Get Claude Code settings path
pub fn get_claude_settings_path() -> PathBuf {
    let dir = get_claude_config_dir();
    let settings = dir.join("settings.json");
    if settings.exists() {
        return settings;
    }
    let legacy = dir.join("claude.json");
    if legacy.exists() {
        return legacy;
    }
    settings
}

/// Get default Claude MCP config path (~/.claude.json)
pub fn get_default_claude_mcp_path() -> PathBuf {
    get_home_dir().join(".claude.json")
}

fn derive_mcp_path_from_override(dir: &Path) -> Option<PathBuf> {
    let file_name = dir
        .file_name()
        .map(|name| name.to_string_lossy().trim().to_string())?;
    if file_name.is_empty() {
        return None;
    }

    let parent = dir.parent().unwrap_or_else(|| Path::new(""));
    Some(parent.join(format!("{file_name}.json")))
}

/// Get Claude MCP config path, honoring override dir when present.
pub fn get_claude_mcp_path() -> PathBuf {
    if let Some(custom_dir) = crate::settings::get_claude_override_dir() {
        if let Some(path) = derive_mcp_path_from_override(&custom_dir) {
            return path;
        }
    }

    get_default_claude_mcp_path()
}

/// Get Codex config directory
pub fn get_codex_config_dir() -> PathBuf {
    crate::settings::get_codex_override_dir().unwrap_or_else(|| get_home_dir().join(".codex"))
}

/// Get Codex auth.json path
pub fn get_codex_auth_path() -> PathBuf {
    get_codex_config_dir().join("auth.json")
}

/// Get Gemini CLI config directory
pub fn get_gemini_config_dir() -> PathBuf {
    crate::settings::get_gemini_override_dir().unwrap_or_else(|| get_home_dir().join(".gemini"))
}

/// Get Gemini .env path
pub fn get_gemini_env_path() -> PathBuf {
    get_gemini_config_dir().join(".env")
}

/// Get OpenCode config directory
pub fn get_opencode_config_dir() -> PathBuf {
    crate::settings::get_opencode_override_dir()
        .unwrap_or_else(|| get_home_dir().join(".config").join("opencode"))
}

/// Get OpenClaw config directory
pub fn get_openclaw_config_dir() -> PathBuf {
    crate::settings::get_openclaw_override_dir().unwrap_or_else(|| get_home_dir().join(".openclaw"))
}

/// Sanitize provider name for file name safety
pub fn sanitize_provider_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '-',
            _ => c,
        })
        .collect::<String>()
        .to_lowercase()
}

/// Read JSON config file
pub fn read_json_file<T: for<'a> Deserialize<'a>>(path: &Path) -> Result<T, AppError> {
    if !path.exists() {
        return Err(AppError::Config(format!(
            "File not found: {}",
            path.display()
        )));
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(path, e))
}

/// Write JSON config file
pub fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json =
        serde_json::to_string_pretty(data).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

/// Write text file (for TOML/plain text)
pub fn write_text_file(path: &Path, data: &str) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    atomic_write(path, data.as_bytes())
}

/// Atomic write: write to temp file then rename, avoid half-written state
pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let parent = path
        .parent()
        .ok_or_else(|| AppError::Config("Invalid path".to_string()))?;
    let mut tmp = parent.to_path_buf();
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::Config("Invalid filename".to_string()))?
        .to_string_lossy()
        .to_string();
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    tmp.push(format!("{file_name}.tmp.{ts}"));

    {
        let mut f = fs::File::create(&tmp).map_err(|e| AppError::io(&tmp, e))?;
        f.write_all(data).map_err(|e| AppError::io(&tmp, e))?;
        f.flush().map_err(|e| AppError::io(&tmp, e))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            let perm = meta.permissions().mode();
            let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(perm));
        }
    }

    #[cfg(windows)]
    {
        if path.exists() {
            let _ = fs::remove_file(path);
        }
        fs::rename(&tmp, path).map_err(|e| AppError::IoContext {
            context: format!(
                "Atomic rename failed: {} -> {}",
                tmp.display(),
                path.display()
            ),
            source: e,
        })?;
    }

    #[cfg(not(windows))]
    {
        fs::rename(&tmp, path).map_err(|e| AppError::IoContext {
            context: format!(
                "Atomic rename failed: {} -> {}",
                tmp.display(),
                path.display()
            ),
            source: e,
        })?;
    }
    Ok(())
}

/// Copy file
pub fn copy_file(from: &Path, to: &Path) -> Result<(), AppError> {
    fs::copy(from, to).map_err(|e| AppError::IoContext {
        context: format!("Copy failed ({} -> {})", from.display(), to.display()),
        source: e,
    })?;
    Ok(())
}

/// Delete file
pub fn delete_file(path: &Path) -> Result<(), AppError> {
    if path.exists() {
        fs::remove_file(path).map_err(|e| AppError::io(path, e))?;
    }
    Ok(())
}

/// Config status
#[derive(Serialize, Deserialize)]
pub struct ConfigStatus {
    pub exists: bool,
    pub path: String,
}

/// Get Claude Code config status
pub fn get_claude_config_status() -> ConfigStatus {
    let path = get_claude_settings_path();
    ConfigStatus {
        exists: path.exists(),
        path: path.to_string_lossy().to_string(),
    }
}
