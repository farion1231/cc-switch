//! MiMo Desktop / MiMoCode configuration management.
//!
//! MiMo Desktop and the MiMoCode engine share the same provider configuration
//! file (`mimocode.jsonc`). This module reads and writes that file, following
//! the same patterns as [`crate::mcode_config`] but adapted for the JSONC
//! format and MiMo-specific structure.
//!
//! The desktop application also stores UI preferences in a separate
//! `preferences.json` (model selection, notification toggles, etc.).

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

// ─── Paths ────────────────────────────────────────────────────────────────────

/// MiMo configuration directory (`~/.config/mimocode`).
pub(crate) fn config_dir() -> PathBuf {
    get_home_dir().join(".config").join("mimocode")
}

/// Path to the shared provider configuration file.
pub(crate) fn config_path() -> PathBuf {
    config_dir().join("mimocode.jsonc")
}

/// Path to the desktop application's preferences file.
pub(crate) fn preferences_path() -> PathBuf {
    let base = dirs::config_dir()
        .or_else(|| dirs::home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| get_home_dir().join(".config"));
    base.join("Xiaomi MiMo").join("preferences.json")
}

// ─── Reading ──────────────────────────────────────────────────────────────────

/// Parse a JSONC source string into a [`Value`].
///
/// Uses `json5` which accepts comments and trailing commas — the same engine
/// [`crate::jsonc_document::JsoncDocument`] uses for format-preserving edits.
fn read_jsonc(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({}));
        }
        Err(e) => return Err(AppError::io(path, e)),
    };
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = json5::from_str(&text)
        .map_err(|e| AppError::Config(format!("Failed to parse MiMo JSONC config: {e}")))?;
    if !value.is_object() {
        return Err(AppError::Config(
            "MiMo configuration root must be a JSON object".into(),
        ));
    }
    Ok(value)
}

fn read_json(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(json!({}));
        }
        Err(e) => return Err(AppError::io(path, e)),
    };
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|e| AppError::Config(format!("Failed to parse MiMo preferences: {e}")))?;
    Ok(value)
}

// ─── Provider CRUD ────────────────────────────────────────────────────────────

/// Returns the `provider` map from `mimocode.jsonc`, or an empty map.
pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read_jsonc(&config_path())?;
    match document.get("provider") {
        None | Some(Value::Null) => Ok(IndexMap::new()),
        Some(value) => {
            let providers: IndexMap<String, Value> =
                serde_json::from_value(value.clone()).map_err(|_| {
                    AppError::Config("Invalid MiMo provider configuration".into())
                })?;
            Ok(providers)
        }
    }
}

/// Checks whether a provider key exists in the live config.
pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    let document = read_jsonc(&config_path())?;
    Ok(document
        .get("provider")
        .and_then(Value::as_object)
        .is_some_and(|providers| providers.contains_key(id)))
}

/// Validates a MiMo provider configuration value.
pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.is_empty()
        || id
            .chars()
            .any(|c| !c.is_ascii_alphanumeric() && !matches!(c, '-' | '_'))
    {
        return Err(AppError::InvalidInput(
            "MiMo provider key must contain letters, digits, '-' or '_'".into(),
        ));
    }
    let base_url = config
        .pointer("/options/baseURL")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !url::Url::parse(base_url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "Enter a valid MiMo endpoint URL".into(),
        ));
    }
    if config
        .pointer("/options/apiKey")
        .and_then(Value::as_str)
        .is_none_or(|key| key.trim().is_empty())
    {
        return Err(AppError::InvalidInput("Enter an API key".into()));
    }
    if config
        .get("models")
        .and_then(Value::as_object)
        .is_none_or(|models| models.is_empty() || models.keys().any(|id| id.trim().is_empty()))
    {
        return Err(AppError::InvalidInput(
            "Add at least one MiMo model".into(),
        ));
    }
    Ok(())
}

// ─── Locking ──────────────────────────────────────────────────────────────────

/// Directory-based lock (same approach as [`crate::mcode_config::ConfigLock`]).
struct ConfigLock(PathBuf);
impl Drop for ConfigLock {
    fn drop(&mut self) {
        let _ = fs::remove_dir(&self.0);
    }
}

fn acquire_lock(path: &Path) -> Result<ConfigLock, AppError> {
    let lock_path = PathBuf::from(format!("{}.lock", path.display()));
    if let Err(error) = fs::create_dir(&lock_path) {
        if error.kind() != std::io::ErrorKind::AlreadyExists {
            return Err(AppError::io(&lock_path, error));
        }
        let stale = fs::metadata(&lock_path)
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|mtime| mtime.elapsed().ok())
            .is_some_and(|age| age > Duration::from_secs(10));
        if !stale {
            return Err(AppError::Conflict(
                "MiMo configuration is busy; retry after MiMo finishes saving".into(),
            ));
        }
        fs::remove_dir(&lock_path).map_err(|e| AppError::io(&lock_path, e))?;
        fs::create_dir(&lock_path).map_err(|e| AppError::io(&lock_path, e))?;
    }
    Ok(ConfigLock(lock_path))
}

fn write_entry(
    path: &Path,
    id: &str,
    provider: Option<Value>,
    create_only: bool,
) -> Result<(), AppError> {
    fs::create_dir_all(path.parent().expect("MiMo configuration directory"))
        .map_err(|e| AppError::Message(format!("Cannot create MiMo directory: {e}")))?;
    let canonical = if path.exists() {
        fs::canonicalize(path).map_err(|e| AppError::Message(e.to_string()))?
    } else {
        path.to_path_buf()
    };
    let _lock = acquire_lock(&canonical)?;
    let mut document = read_jsonc(&canonical)?;
    let root = document
        .as_object_mut()
        .ok_or_else(|| AppError::Config("MiMo configuration root must be an object".into()))?;
    let entry = root
        .entry("provider".to_string())
        .or_insert_with(|| Value::Object(serde_json::Map::new()));
    let providers = entry
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Invalid MiMo provider configuration".into()))?;
    if create_only && providers.contains_key(id) {
        return Err(AppError::InvalidInput(format!(
            "MiMo provider key '{id}' already exists"
        )));
    }
    if let Some(provider) = provider {
        providers.insert(id.to_string(), provider);
    } else {
        providers.remove(id);
    }
    let json = serde_json::to_string_pretty(&document)
        .map_err(|_| AppError::Config("Cannot serialize MiMo configuration".into()))?;
    atomic_write_private(path, json.as_bytes())
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    write_entry(&config_path(), id, Some(config), false)
}

pub(crate) fn add_provider(id: &str, config: Value) -> Result<(), AppError> {
    validate_provider(id, &config)?;
    write_entry(&config_path(), id, Some(config), true)
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    if !config_path().exists() {
        return Ok(());
    }
    write_entry(&config_path(), id, None, false)
}

// ─── Preferences ──────────────────────────────────────────────────────────────

/// Returns the parsed `preferences.json`, or an empty object.
pub(crate) fn get_preferences() -> Result<Value, AppError> {
    read_json(&preferences_path())
}

/// Atomically writes `preferences.json`.
pub(crate) fn set_preferences(preferences: &Value) -> Result<(), AppError> {
    let path = preferences_path();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| AppError::Message(format!("Cannot create MiMo preferences dir: {e}")))?;
    }
    let json = serde_json::to_string_pretty(preferences)
        .map_err(|_| AppError::Config("Cannot serialize MiMo preferences".into()))?;
    atomic_write_private(&path, json.as_bytes())
}

/// Returns the currently selected model string (e.g. `"quickapi/dots3-note-prev"`).
pub(crate) fn current_model() -> Result<String, AppError> {
    let prefs = get_preferences()?;
    Ok(prefs
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string())
}

/// Sets the currently selected model string in `preferences.json`.
pub(crate) fn set_current_model(model: &str) -> Result<(), AppError> {
    let mut prefs = get_preferences()?;
    if let Some(obj) = prefs.as_object_mut() {
        obj.insert("model".to_string(), Value::String(model.to_string()));
    }
    set_preferences(&prefs)
}

/// Returns the notification settings object from `preferences.json`.
pub(crate) fn get_task_notifications() -> Result<Value, AppError> {
    let prefs = get_preferences()?;
    Ok(prefs.get("taskNotifications").cloned().unwrap_or_else(|| json!({
        "completion": "unfocused",
        "permission": true,
        "question": true,
        "failure": true,
    })))
}

/// Sets the notification settings object in `preferences.json`.
pub(crate) fn set_task_notifications(notifications: &Value) -> Result<(), AppError> {
    let mut prefs = get_preferences()?;
    if let Some(obj) = prefs.as_object_mut() {
        obj.insert("taskNotifications".to_string(), notifications.clone());
    }
    set_preferences(&prefs)
}

// ─── Atomic write + commit ────────────────────────────────────────────────────

/// Callers hold their feature lock across the native write and database commit.
pub(crate) fn write_and_commit<T>(
    path: &Path,
    write: impl FnOnce() -> Result<(), AppError>,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let previous = match fs::read(path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::io(path, error)),
    };
    write()?;
    match commit() {
        Ok(result) => Ok(result),
        Err(error) => {
            let rollback = match previous {
                Some(bytes) => atomic_write_private(path, &bytes),
                None => fs::remove_file(path).map_err(|error| AppError::io(path, error)),
            };
            if let Err(rollback_error) = rollback {
                return Err(AppError::Message(format!(
                    "Config update failed ({error}); restoring {} also failed: {rollback_error}",
                    path.display()
                )));
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_dir_uses_xdg_home() {
        // On all platforms, the config dir is ~/.config/mimocode
        let dir = config_dir();
        assert!(dir.ends_with(".config/mimocode"));
    }

    #[test]
    fn validate_provider_rejects_empty_key() {
        assert!(validate_provider("", &json!({"options":{"baseURL":"https://x.com","apiKey":"k"},"models":{"m":{}}})).is_err());
    }

    #[test]
    fn validate_provider_rejects_bad_url() {
        assert!(validate_provider("ok", &json!({"options":{"baseURL":"ftp://x.com","apiKey":"k"},"models":{"m":{}}})).is_err());
    }

    #[test]
    fn validate_provider_rejects_missing_key() {
        assert!(validate_provider("ok", &json!({"options":{"baseURL":"https://x.com"},"models":{"m":{}}})).is_err());
    }

    #[test]
    fn validate_provider_rejects_no_models() {
        assert!(validate_provider("ok", &json!({"options":{"baseURL":"https://x.com","apiKey":"k"},"models":{}})).is_err());
    }

    #[test]
    fn validate_provider_accepts_valid_config() {
        assert!(validate_provider("ok", &json!({"options":{"baseURL":"https://x.com","apiKey":"k"},"models":{"m":{}}})).is_ok());
    }
}