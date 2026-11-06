//! WorkBuddy's user-level model list (`~/.workbuddy-ai/models.json`).
//!
//! WorkBuddy keeps every custom model as one entry of the `models` array and
//! uses `availableModels` only as a visibility filter for its model picker.
//! `availableModels` is **not** the currently selected model: WorkBuddy picks a
//! model per conversation in its own UI and has no file-level "active model"
//! switch. CC Switch therefore never claims one, and enabling a card here means
//! "show this model in WorkBuddy", not "use this model".
//!
//! One CC Switch provider is one `models[]` entry:
//!
//! * `provider.id` == `models[i].id` (also the model name sent upstream)
//! * `provider.name` == `models[i].name` (display name)
//! * `provider.settings_config` == the whole `models[i]` object, so fields CC
//!   Switch does not know about survive every read/modify/write cycle.
//!
//! Everything else in the document is preserved as well: unknown root keys,
//! unknown keys inside a model, and ids listed in `availableModels` that no
//! model owns. The file is rewritten only through [`atomic_write_private`] and
//! only after a revision check, so an external edit is reported instead of
//! silently overwritten.

use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex, MutexGuard};

/// WorkBuddy refuses to load a runaway file; stay well inside that bound.
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MISSING_REVISION: &str = "missing";
/// Backups follow the same layout as every other CC Switch config backup:
/// `<app config dir>/backups/workbuddy/workbuddy_<timestamp>.json`.
const BACKUP_PREFIX: &str = "workbuddy_";

static MODELS_FILE_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));
#[cfg(test)]
static TEST_CONFIG_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));
/// Tests must never write into the real `~/.cc-switch/backups`.
#[cfg(test)]
static TEST_BACKUP_DIR: LazyLock<Mutex<Option<PathBuf>>> = LazyLock::new(|| Mutex::new(None));

/// WorkBuddy's user-level config directory: the CC Switch override, else
/// `~/.workbuddy-ai` (Windows: `%USERPROFILE%\.workbuddy-ai`).
pub(crate) fn get_workbuddy_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_CONFIG_DIR
        .lock()
        .expect("lock WorkBuddy test directory")
        .clone()
    {
        return Ok(path);
    }

    if let Some(dir) = crate::settings::get_workbuddy_override_dir() {
        return Ok(dir);
    }

    Ok(get_home_dir().join(".workbuddy-ai"))
}

pub(crate) fn get_models_path() -> Result<PathBuf, AppError> {
    Ok(get_workbuddy_dir()?.join("models.json"))
}

fn backup_dir() -> Result<PathBuf, AppError> {
    #[cfg(test)]
    if let Some(path) = TEST_BACKUP_DIR
        .lock()
        .expect("lock WorkBuddy test backup directory")
        .clone()
    {
        return Ok(path);
    }

    Ok(crate::config::get_app_config_dir()
        .join("backups")
        .join("workbuddy"))
}

/// One consistent read of both halves of the file.
#[derive(Debug)]
pub(crate) struct ModelsSnapshot {
    pub models: IndexMap<String, Value>,
    pub available: Option<Vec<String>>,
}

pub(crate) fn read_snapshot() -> Result<ModelsSnapshot, AppError> {
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let document = read_models_document(&path)?;
    let available = available_models(&document, &path)?;
    let models = models(&document, &path)?
        .into_iter()
        .filter_map(|model| model_id(model).map(|id| (id.to_string(), model.clone())))
        .collect();
    Ok(ModelsSnapshot { models, available })
}

pub(crate) fn read_model(model_key: &str) -> Result<Option<Value>, AppError> {
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let document = read_models_document(&path)?;
    Ok(models(&document, &path)?
        .into_iter()
        .find(|model| model_id(model) == Some(model_key))
        .cloned())
}

/// The visibility filter as written in the file. `None` means the key is
/// absent, which WorkBuddy treats as "every model is available".
pub(crate) fn read_available_models() -> Result<Option<Vec<String>>, AppError> {
    Ok(read_snapshot()?.available)
}

pub(crate) fn model_exists(model_key: &str) -> Result<bool, AppError> {
    Ok(read_model(model_key)?.is_some())
}

/// Whether `model_key` currently shows up in WorkBuddy's model picker.
pub(crate) fn is_available(available: Option<&[String]>, model_key: &str) -> bool {
    match available {
        None => true,
        Some(ids) => ids.iter().any(|id| id == model_key),
    }
}

/// Validate one `models[]` node before CC Switch persists it.
///
/// Unknown fields are explicitly allowed: WorkBuddy grows this schema over
/// time and CC Switch must not be the reason a new field disappears.
pub(crate) fn validate_model_node(model_key: &str, config: &Value) -> Result<(), AppError> {
    let key = model_key.trim();
    if key.is_empty() {
        return Err(AppError::InvalidInput(
            "WorkBuddy model id cannot be empty".to_string(),
        ));
    }
    let object = config.as_object().ok_or_else(|| {
        AppError::InvalidInput("WorkBuddy model configuration must be an object".to_string())
    })?;
    match object.get("id") {
        Some(Value::String(id)) if id.trim() == key => {}
        Some(Value::String(_)) => {
            return Err(AppError::InvalidInput(format!(
                "WorkBuddy model id '{key}' does not match the 'id' field of its configuration"
            )))
        }
        Some(_) => {
            return Err(AppError::InvalidInput(
                "WorkBuddy model 'id' must be a string".to_string(),
            ))
        }
        None => {
            return Err(AppError::InvalidInput(
                "WorkBuddy model configuration needs an 'id'".to_string(),
            ))
        }
    }
    if let Some(name) = object.get("name") {
        if !name.is_string() {
            return Err(AppError::InvalidInput(
                "WorkBuddy model 'name' must be a string".to_string(),
            ));
        }
    }
    for key in ["url", "apiKey", "vendor"] {
        if let Some(value) = object.get(key) {
            if !value.is_string() {
                return Err(AppError::InvalidInput(format!(
                    "WorkBuddy model '{key}' must be a string"
                )));
            }
        }
    }
    match object.get("url").and_then(Value::as_str) {
        Some(url) if !url.trim().is_empty() => {}
        _ => {
            return Err(AppError::InvalidInput(
                "WorkBuddy model needs a request URL ('url')".to_string(),
            ))
        }
    }
    for key in [
        "maxInputTokens",
        "maxOutputTokens",
        "supportsToolCall",
        "supportsImages",
        "supportsReasoning",
    ] {
        if let Some(value) = object.get(key) {
            if !value.is_number() && !value.is_boolean() {
                return Err(AppError::InvalidInput(format!(
                    "WorkBuddy model '{key}' must be a number or a boolean"
                )));
            }
        }
    }
    if let Some(value) = object.get("relatedModels") {
        let valid = value
            .as_array()
            .is_some_and(|items| items.iter().all(Value::is_string));
        if !valid {
            return Err(AppError::InvalidInput(
                "WorkBuddy model 'relatedModels' must be an array of strings".to_string(),
            ));
        }
    }
    Ok(())
}

pub(crate) fn model_base_url(config: &Value) -> Result<String, AppError> {
    config
        .get("url")
        .and_then(Value::as_str)
        .filter(|url| !url.trim().is_empty())
        .map(str::to_string)
        .ok_or_else(|| AppError::InvalidInput("WorkBuddy model has no request URL".to_string()))
}

pub(crate) fn model_api_key(config: &Value) -> String {
    config
        .get("apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Append one model. Returns `Ok(false)` when an identical entry already exists.
pub(crate) fn insert_model(model_key: &str, config: &Value) -> Result<bool, AppError> {
    validate_model_node(model_key, config)?;
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;

    let existing = models(&document, &path)?
        .into_iter()
        .find(|model| model_id(model) == Some(model_key))
        .cloned();
    if let Some(existing) = existing {
        if existing == *config {
            return Ok(false);
        }
        return Err(AppError::InvalidInput(format!(
            "WorkBuddy model id '{model_key}' already exists in models.json"
        )));
    }

    models_mut(&mut document, &path)?.push(config.clone());
    // A model CC Switch just added is meant to be usable right away; when the
    // file carries a visibility list, keep it in sync. A file without the key
    // stays without it — every model is available in that shape.
    if let Some(available) = available_models_mut(&mut document, false)? {
        if !available.iter().any(|id| id == model_key) {
            available.push(Value::String(model_key.to_string()));
        }
    }
    write_models_document(&path, &document, &expected_revision)?;
    Ok(true)
}

/// Replace one model in place. The key must not change: it is the model name
/// WorkBuddy sends upstream, so renaming is a delete plus a create.
pub(crate) fn replace_model(
    model_key: &str,
    expected: &Value,
    replacement: &Value,
) -> Result<(), AppError> {
    validate_model_node(model_key, replacement)?;
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let index = index_of(&document, &path, model_key)?.ok_or_else(|| {
        AppError::Conflict(format!(
            "WorkBuddy model '{model_key}' is no longer present in models.json"
        ))
    })?;
    let current = models(&document, &path)?[index].clone();
    if current != *expected {
        return Err(AppError::Conflict(format!(
            "WorkBuddy model '{model_key}' changed outside CC Switch"
        )));
    }
    if current == *replacement {
        return Ok(());
    }
    models_mut(&mut document, &path)?[index] = replacement.clone();
    write_models_document(&path, &document, &expected_revision)
}

/// Replace a model only when it is still present; returns the previous value.
pub(crate) fn replace_model_if_present(
    model_key: &str,
    replacement: &Value,
) -> Result<Option<Value>, AppError> {
    validate_model_node(model_key, replacement)?;
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let Some(index) = index_of(&document, &path, model_key)? else {
        return Ok(None);
    };
    let current = models(&document, &path)?[index].clone();
    if current == *replacement {
        return Ok(Some(current));
    }
    models_mut(&mut document, &path)?[index] = replacement.clone();
    write_models_document(&path, &document, &expected_revision)?;
    Ok(Some(current))
}

/// Removed model plus whether it was visible before the call.
pub(crate) struct RemovedModel {
    pub config: Value,
    pub was_available: bool,
}

pub(crate) fn remove_model(model_key: &str) -> Result<Option<RemovedModel>, AppError> {
    remove_model_inner(model_key, None)
}

pub(crate) fn remove_model_if_matches(model_key: &str, expected: &Value) -> Result<bool, AppError> {
    remove_model_inner(model_key, Some(expected)).map(|removed| removed.is_some())
}

fn remove_model_inner(
    model_key: &str,
    expected: Option<&Value>,
) -> Result<Option<RemovedModel>, AppError> {
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let Some(index) = index_of(&document, &path, model_key)? else {
        return Ok(None);
    };
    let current = models(&document, &path)?[index].clone();
    if expected.is_some_and(|expected| current != *expected) {
        return Err(AppError::Conflict(format!(
            "WorkBuddy model '{model_key}' changed outside CC Switch"
        )));
    }
    let was_available = {
        let available = available_models(&document, &path)?;
        is_available(available.as_deref(), model_key)
    };
    models_mut(&mut document, &path)?.remove(index);
    if let Some(available) = available_models_mut(&mut document, false)? {
        available.retain(|id| id.as_str() != Some(model_key));
    }
    write_models_document(&path, &document, &expected_revision)?;
    Ok(Some(RemovedModel {
        config: current,
        was_available,
    }))
}

/// Put a removed model back, keeping the visibility it had before removal.
pub(crate) fn restore_model(
    model_key: &str,
    config: &Value,
    was_available: bool,
) -> Result<(), AppError> {
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    let list = models_mut(&mut document, &path)?;
    match list
        .iter()
        .position(|model| model_id(model) == Some(model_key))
    {
        Some(index) if list[index] == *config => {}
        Some(_) => {
            return Err(AppError::Conflict(format!(
                "cannot restore WorkBuddy model '{model_key}' because another entry now owns the id"
            )))
        }
        None => list.push(config.clone()),
    }
    if was_available {
        if let Some(available) = available_models_mut(&mut document, false)? {
            if !available.iter().any(|id| id.as_str() == Some(model_key)) {
                available.push(Value::String(model_key.to_string()));
            }
        }
    }
    write_models_document(&path, &document, &expected_revision)
}

/// Add or remove one id in `availableModels`.
///
/// A file without the key means every model is visible, so the first toggle
/// materializes the list from the models that exist at that moment. Ids that no
/// model owns are never dropped.
pub(crate) fn set_model_available(model_key: &str, available: bool) -> Result<bool, AppError> {
    let _guard = lock_models_file()?;
    let path = get_models_path()?;
    let (mut document, expected_revision) = read_models_document_with_revision(&path)?;
    if !models(&document, &path)?
        .iter()
        .any(|model| model_id(model) == Some(model_key))
    {
        return Err(AppError::InvalidInput(format!(
            "WorkBuddy model '{model_key}' is not in models.json"
        )));
    }

    let list = available_models_mut(&mut document, true)?.expect("visibility list must exist");
    let present = list.iter().any(|id| id.as_str() == Some(model_key));
    if present == available {
        return Ok(false);
    }
    if available {
        list.push(Value::String(model_key.to_string()));
    } else {
        list.retain(|id| id.as_str() != Some(model_key));
    }
    write_models_document(&path, &document, &expected_revision)?;
    Ok(true)
}

/// Write the newest backup back over `models.json`. Returns `false` when there
/// is no backup to restore from.
///
/// There is no revision anchor for this direction: the backup we are about to
/// apply was taken before the last write, so "the file changed since" is exactly
/// what this call is meant to undo. To keep that from costing a user their own
/// edits, the state being replaced is backed up first — a restore is itself
/// undoable, and the same retention limit applies.
pub(crate) fn restore_backup() -> Result<bool, AppError> {
    let _guard = lock_models_file()?;
    let Some(backup) = latest_backup()? else {
        return Ok(false);
    };
    let bytes = read_file_limited(&backup, "WorkBuddy models backup")?;
    // Refuse to restore a file WorkBuddy could not read: valid JSON alone is not
    // enough, `models` and `availableModels` must still have their own shapes.
    let document = parse_models_value(&backup, bytes.clone())?;
    models(&document, &backup)?;
    available_models(&document, &backup)?;

    let path = get_models_path()?;
    ensure_private_parent(&path)?;
    back_up_existing(&path)?;
    atomic_write_private(&path, &bytes)?;
    Ok(true)
}

pub(crate) fn backup_exists() -> Result<bool, AppError> {
    Ok(latest_backup()?.is_some())
}

/// Backups, oldest first: the file name uses a fixed-width timestamp, so sorting
/// by name is sorting by age.
fn backup_entries() -> Result<Vec<PathBuf>, AppError> {
    let dir = backup_dir()?;
    let entries = match fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(AppError::io(&dir, error)),
    };

    let mut paths: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension().is_some_and(|ext| ext == "json")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().starts_with(BACKUP_PREFIX))
        })
        .collect();
    paths.sort();
    Ok(paths)
}

fn latest_backup() -> Result<Option<PathBuf>, AppError> {
    Ok(backup_entries()?.pop())
}

fn lock_models_file() -> Result<MutexGuard<'static, ()>, AppError> {
    MODELS_FILE_LOCK
        .lock()
        .map_err(|error| AppError::Lock(format!("WorkBuddy models file lock is poisoned: {error}")))
}

fn read_models_document(path: &Path) -> Result<Value, AppError> {
    read_models_document_with_revision(path).map(|(document, _)| document)
}

fn read_models_document_with_revision(path: &Path) -> Result<(Value, String), AppError> {
    match fs::File::open(path) {
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            // A missing file is a valid empty document: WorkBuddy creates it on
            // first save, and CC Switch must be able to seed it safely.
            return Ok((Value::Object(Map::new()), MISSING_REVISION.to_string()));
        }
        Err(error) => return Err(AppError::io(path, error)),
    }
    let bytes = read_file_limited(path, "WorkBuddy models")?;
    let revision = revision(&bytes);
    Ok((parse_models_value(path, bytes)?, revision))
}

fn read_file_limited(path: &Path, label: &str) -> Result<Vec<u8>, AppError> {
    let file = fs::File::open(path).map_err(|error| AppError::io(path, error))?;
    let metadata = file.metadata().map_err(|error| AppError::io(path, error))?;
    if metadata.len() > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| AppError::io(path, error))?;
    if bytes.len() as u64 > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "{label} file exceeds the 1 MiB limit: {}",
            path.display()
        )));
    }
    Ok(bytes)
}

fn parse_models_value(path: &Path, bytes: Vec<u8>) -> Result<Value, AppError> {
    // WorkBuddy reads plain JSON (no JSON5/JSONC), so a comment or a trailing
    // comma is a real error there and must be reported here too instead of
    // being silently repaired and written back in a different shape.
    let value: Value = serde_json::from_slice(&bytes).map_err(|error| {
        AppError::Config(format!(
            "WorkBuddy models file is not valid JSON ({}): {error}",
            path.display()
        ))
    })?;
    if !value.is_object() {
        return Err(AppError::Config(format!(
            "WorkBuddy models root must be an object: {}",
            path.display()
        )));
    }
    Ok(value)
}

/// All `models[]` entries, validated: every entry must be an object with a
/// unique, non-empty string `id`.
fn models<'a>(document: &'a Value, path: &Path) -> Result<Vec<&'a Value>, AppError> {
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "WorkBuddy models root must be an object: {}",
            path.display()
        ))
    })?;
    let entries = match root.get("models") {
        None | Some(Value::Null) => return Ok(Vec::new()),
        Some(Value::Array(entries)) => entries,
        Some(_) => {
            return Err(AppError::Config(format!(
                "WorkBuddy models 'models' must be an array: {}",
                path.display()
            )))
        }
    };

    let mut seen: Vec<&str> = Vec::with_capacity(entries.len());
    let mut result = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        if !entry.is_object() {
            return Err(AppError::Config(format!(
                "WorkBuddy models[{}] must be an object: {}",
                index,
                path.display()
            )));
        }
        let id = model_id(entry).ok_or_else(|| {
            AppError::Config(format!(
                "WorkBuddy models[{}] has no string 'id': {}",
                index,
                path.display()
            ))
        })?;
        if id.trim().is_empty() {
            return Err(AppError::Config(format!(
                "WorkBuddy models[{}] has an empty 'id': {}",
                index,
                path.display()
            )));
        }
        if seen.contains(&id) {
            return Err(AppError::Config(format!(
                "WorkBuddy models.json contains duplicate model id '{id}'; fix the duplicates before CC Switch can manage them: {}",
                path.display()
            )));
        }
        seen.push(id);
        result.push(entry);
    }
    Ok(result)
}

fn models_mut<'a>(document: &'a mut Value, path: &Path) -> Result<&'a mut Vec<Value>, AppError> {
    let root = document.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "WorkBuddy models root must be an object: {}",
            path.display()
        ))
    })?;
    let value = root
        .entry("models".to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if value.is_null() {
        *value = Value::Array(Vec::new());
    }
    value.as_array_mut().ok_or_else(|| {
        AppError::Config(format!(
            "WorkBuddy models 'models' must be an array: {}",
            path.display()
        ))
    })
}

fn model_id(model: &Value) -> Option<&str> {
    model.get("id").and_then(Value::as_str)
}

fn index_of(document: &Value, path: &Path, model_key: &str) -> Result<Option<usize>, AppError> {
    Ok(models(document, path)?
        .iter()
        .position(|model| model_id(model) == Some(model_key)))
}

fn available_models(document: &Value, path: &Path) -> Result<Option<Vec<String>>, AppError> {
    let root = document.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "WorkBuddy models root must be an object: {}",
            path.display()
        ))
    })?;
    match root.get("availableModels") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Array(items)) => {
            let mut ids = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let id = item.as_str().ok_or_else(|| {
                    AppError::Config(format!(
                        "WorkBuddy availableModels[{}] must be a string: {}",
                        index,
                        path.display()
                    ))
                })?;
                ids.push(id.to_string());
            }
            Ok(Some(ids))
        }
        Some(_) => Err(AppError::Config(format!(
            "WorkBuddy models 'availableModels' must be an array: {}",
            path.display()
        ))),
    }
}

/// Mutable visibility list. `create` materializes it from the models that exist
/// right now, which keeps a document without the key behaving as "every model
/// is available" until the user actually hides something.
fn available_models_mut(
    document: &mut Value,
    create: bool,
) -> Result<Option<&mut Vec<Value>>, AppError> {
    let root = document
        .as_object_mut()
        .ok_or_else(|| AppError::Config("WorkBuddy models root must be an object".to_string()))?;

    let key_present = root
        .get("availableModels")
        .is_some_and(|value| !value.is_null());
    if !key_present {
        if !create {
            return Ok(None);
        }
        let known: Vec<Value> = root
            .get("models")
            .and_then(Value::as_array)
            .map(|entries| {
                entries
                    .iter()
                    .filter_map(model_id)
                    .map(|id| Value::String(id.to_string()))
                    .collect()
            })
            .unwrap_or_default();
        root.insert("availableModels".to_string(), Value::Array(known));
    }

    root.get_mut("availableModels")
        .and_then(Value::as_array_mut)
        .map(Some)
        .ok_or_else(|| {
            AppError::Config("WorkBuddy models 'availableModels' must be an array".to_string())
        })
}

fn write_models_document(
    path: &Path,
    document: &Value,
    expected_revision: &str,
) -> Result<(), AppError> {
    let mut bytes =
        serde_json::to_vec_pretty(document).map_err(|source| AppError::JsonSerialize { source })?;
    bytes.push(b'\n');
    ensure_private_parent(path)?;
    ensure_revision(path, expected_revision)?;
    back_up_existing(path)?;
    atomic_write_private(path, &bytes)
}

/// Keep the content of the file about to be replaced. A user who edits the same
/// file in WorkBuddy, or a CC Switch bug, must never cost the original bytes.
fn back_up_existing(path: &Path) -> Result<(), AppError> {
    if !path.exists() {
        return Ok(());
    }
    let dir = backup_dir()?;
    fs::create_dir_all(&dir).map_err(|error| AppError::io(&dir, error))?;

    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let mut backup = dir.join(format!("{BACKUP_PREFIX}{stamp}.json"));
    let mut counter = 1;
    while backup.exists() {
        backup = dir.join(format!("{BACKUP_PREFIX}{stamp}_{counter}.json"));
        counter += 1;
    }

    let bytes = read_file_limited(path, "WorkBuddy models")?;
    atomic_write_private(&backup, &bytes)?;
    prune_backups()
}

/// Keep the newest `backup_retain_count` backups, the same setting every other
/// CC Switch config backup honours.
fn prune_backups() -> Result<(), AppError> {
    prune_backups_to(crate::settings::effective_backup_retain_count())
}

/// The retention step on its own: the limit comes from the user's settings, which
/// a test must not have to write, so the caller passes it in.
fn prune_backups_to(retain: usize) -> Result<(), AppError> {
    let retain = retain.max(1);
    let entries = backup_entries()?;
    if entries.len() <= retain {
        return Ok(());
    }
    let remove_count = entries.len() - retain;
    for path in entries.into_iter().take(remove_count) {
        if let Err(error) = fs::remove_file(&path) {
            log::warn!(
                "Failed to remove old WorkBuddy backup {}: {error}",
                path.display()
            );
        }
    }
    Ok(())
}

fn ensure_revision(path: &Path, expected_revision: &str) -> Result<(), AppError> {
    let actual = match fs::File::open(path) {
        Ok(_) => revision(&read_file_limited(path, "WorkBuddy models")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => MISSING_REVISION.to_string(),
        Err(error) => return Err(AppError::io(path, error)),
    };
    if actual == expected_revision {
        Ok(())
    } else {
        Err(AppError::Conflict(format!(
            "WorkBuddy models.json changed outside CC Switch: {}",
            path.display()
        )))
    }
}

fn revision(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn ensure_private_parent(path: &Path) -> Result<(), AppError> {
    let parent = path.parent().ok_or_else(|| {
        AppError::Config(format!(
            "WorkBuddy models path has no parent directory: {}",
            path.display()
        ))
    })?;
    let created = !parent.exists();
    fs::create_dir_all(parent).map_err(|source| AppError::io(parent, source))?;

    #[cfg(not(unix))]
    let _ = created;

    #[cfg(unix)]
    if created {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700))
            .map_err(|source| AppError::io(parent, source))?;
    }

    Ok(())
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::PathBuf;

    pub(crate) struct TestConfigDir {
        /// Owns the directory, so it lives exactly as long as the override.
        _dir: tempfile::TempDir,
        previous: Option<PathBuf>,
        previous_backup: Option<PathBuf>,
    }

    impl TestConfigDir {
        pub(crate) fn new() -> Self {
            let dir = tempfile::tempdir().expect("create WorkBuddy test directory");
            let previous = super::TEST_CONFIG_DIR
                .lock()
                .expect("lock WorkBuddy test directory")
                .replace(dir.path().to_path_buf());
            let previous_backup = super::TEST_BACKUP_DIR
                .lock()
                .expect("lock WorkBuddy test backup directory")
                .replace(dir.path().join("backups"));
            Self {
                _dir: dir,
                previous,
                previous_backup,
            }
        }
    }

    impl Drop for TestConfigDir {
        fn drop(&mut self) {
            *super::TEST_CONFIG_DIR
                .lock()
                .expect("lock WorkBuddy test directory") = self.previous.take();
            *super::TEST_BACKUP_DIR
                .lock()
                .expect("lock WorkBuddy test backup directory") = self.previous_backup.take();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::TestConfigDir;
    use super::*;
    use serde_json::json;
    use serial_test::serial;

    fn model(id: &str) -> Value {
        json!({
            "id": id,
            "name": format!("Model {id}"),
            "vendor": "custom",
            "apiKey": "test-key",
            "url": "https://api.example.com/v1/chat/completions",
            "maxInputTokens": 128000,
            "maxOutputTokens": 8192,
            "supportsToolCall": true,
            "supportsImages": false,
            "supportsReasoning": false,
            "relatedModels": []
        })
    }

    fn write_file(contents: &str) {
        let path = get_models_path().unwrap();
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }

    #[test]
    #[serial]
    fn missing_file_reads_as_empty_and_first_insert_creates_it() {
        let _dir = TestConfigDir::new();
        assert!(read_snapshot().unwrap().models.is_empty());
        assert_eq!(read_available_models().unwrap(), None);

        assert!(insert_model("model-a", &model("model-a")).unwrap());
        let path = get_models_path().unwrap();
        assert!(path.exists());

        let document: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(document["models"].as_array().unwrap().len(), 1);
        // No visibility list existed, so none is invented.
        assert!(document.get("availableModels").is_none());
        assert_eq!(
            read_snapshot().unwrap().models.keys().collect::<Vec<_>>(),
            ["model-a"]
        );
    }

    #[test]
    #[serial]
    fn existing_models_are_read_in_file_order() {
        let _dir = TestConfigDir::new();
        write_file(
            r#"{"models":[{"id":"b","name":"B","url":"https://b"}],"availableModels":["b"]}"#,
        );
        write_file(
            r#"{"models":[{"id":"a","url":"https://a"},{"id":"b","url":"https://b"}],"availableModels":["b","a"]}"#,
        );

        let models = read_snapshot().unwrap().models;
        assert_eq!(models.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(
            read_available_models().unwrap(),
            Some(vec!["b".to_string(), "a".to_string()])
        );
    }

    #[test]
    #[serial]
    fn insert_appends_and_respects_the_visibility_list() {
        let _dir = TestConfigDir::new();
        write_file(r#"{"models":[],"availableModels":["keep-me"]}"#);

        insert_model("model-a", &model("model-a")).unwrap();
        let document: Value =
            serde_json::from_str(&fs::read_to_string(get_models_path().unwrap()).unwrap()).unwrap();
        assert_eq!(document["models"][0]["id"], "model-a");
        // Ids no model owns stay untouched, the new model becomes visible.
        assert_eq!(document["availableModels"], json!(["keep-me", "model-a"]));
    }

    #[test]
    #[serial]
    fn insert_rejects_duplicate_ids_and_invalid_nodes() {
        let _dir = TestConfigDir::new();
        write_file(r#"{"models":[{"id":"dup","url":"https://a"}],"availableModels":[]}"#);

        let error = insert_model("dup", &model("dup")).expect_err("duplicate id");
        assert!(matches!(error, AppError::InvalidInput(_)));
        assert!(error.to_string().contains("already exists"));

        let error = insert_model("other", &json!({"url": "https://a"})).expect_err("no id");
        assert!(error.to_string().contains("needs an 'id'"));

        let error = insert_model("x", &json!({"id": "x"})).expect_err("no url");
        assert!(error.to_string().contains("request URL"));

        let error = validate_model_node("x", &json!({"id": "y", "url": "https://a"}))
            .expect_err("mismatched id");
        assert!(error.to_string().contains("does not match"));
    }

    #[test]
    #[serial]
    fn invalid_json_is_reported_with_the_path() {
        let _dir = TestConfigDir::new();
        write_file("{\"models\": [}");

        let error = read_snapshot().expect_err("invalid JSON");
        assert!(matches!(error, AppError::Config(_)));
        let message = error.to_string();
        assert!(message.contains("not valid JSON"));
        assert!(message.contains("models.json"));
    }

    #[test]
    #[serial]
    fn duplicate_ids_in_the_file_are_reported() {
        let _dir = TestConfigDir::new();
        write_file(r#"{"models":[{"id":"dup","url":"https://a"},{"id":"dup","url":"https://b"}]}"#);

        let error = read_snapshot().expect_err("duplicate ids in file");
        assert!(error.to_string().contains("duplicate model id 'dup'"));
    }

    #[test]
    #[serial]
    fn unknown_fields_survive_every_write() {
        let _dir = TestConfigDir::new();
        write_file(
            r#"{
                "models": [{
                    "id": "model-a",
                    "url": "https://api.example.com/v1",
                    "futureField": {"nested": [1, 2, 3]}
                }],
                "availableModels": ["model-a"],
                "futureRootKey": "kept"
            }"#,
        );

        replace_model(
            "model-a",
            &json!({
                "id": "model-a",
                "url": "https://api.example.com/v1",
                "futureField": {"nested": [1, 2, 3]}
            }),
            &json!({
                "id": "model-a",
                "name": "Renamed",
                "url": "https://api.example.com/v1",
                "futureField": {"nested": [1, 2, 3]}
            }),
        )
        .unwrap();

        let document: Value =
            serde_json::from_str(&fs::read_to_string(get_models_path().unwrap()).unwrap()).unwrap();
        assert_eq!(document["models"][0]["name"], "Renamed");
        assert_eq!(
            document["models"][0]["futureField"],
            json!({"nested": [1, 2, 3]})
        );
        assert_eq!(document["futureRootKey"], "kept");
        assert_eq!(document["availableModels"], json!(["model-a"]));
    }

    #[test]
    #[serial]
    fn replace_refuses_when_the_file_changed_underneath() {
        let _dir = TestConfigDir::new();
        insert_model("model-a", &model("model-a")).unwrap();
        let original = read_model("model-a").unwrap().unwrap();

        // External edit between CC Switch's read and its write.
        let mut external = original.clone();
        external["name"] = json!("Edited outside");
        let path = get_models_path().unwrap();
        let mut document: Value =
            serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        document["models"][0] = external;
        fs::write(&path, serde_json::to_string(&document).unwrap()).unwrap();

        let error = replace_model("model-a", &original, &model("model-a"))
            .expect_err("stale write must be refused");
        assert!(matches!(error, AppError::Conflict(_)));
        assert!(error.to_string().contains("changed outside CC Switch"));
    }

    #[test]
    #[serial]
    fn availability_toggles_materialize_the_list_and_keep_foreign_ids() {
        let _dir = TestConfigDir::new();
        write_file(
            r#"{"models":[{"id":"a","url":"https://a"},{"id":"b","url":"https://b"}],"availableModels":["a","b","foreign"]}"#,
        );

        assert!(set_model_available("b", false).unwrap());
        assert_eq!(
            read_available_models().unwrap(),
            Some(vec!["a".to_string(), "foreign".to_string()])
        );
        assert!(!is_available(
            read_available_models().unwrap().as_deref(),
            "b"
        ));

        assert!(set_model_available("b", true).unwrap());
        assert!(is_available(
            read_available_models().unwrap().as_deref(),
            "b"
        ));
        // Toggling twice is a no-op the second time.
        assert!(!set_model_available("b", true).unwrap());

        let error = set_model_available("missing", true).expect_err("unknown model");
        assert!(error.to_string().contains("not in models.json"));
    }

    #[test]
    #[serial]
    fn absent_visibility_list_means_every_model_is_visible() {
        let _dir = TestConfigDir::new();
        insert_model("a", &model("a")).unwrap();
        insert_model("b", &model("b")).unwrap();
        assert!(is_available(
            read_available_models().unwrap().as_deref(),
            "a"
        ));

        // Hiding one model materializes the list so the others stay visible.
        assert!(set_model_available("b", false).unwrap());
        assert_eq!(
            read_available_models().unwrap(),
            Some(vec!["a".to_string()])
        );
    }

    #[test]
    #[serial]
    fn remove_drops_the_entry_and_its_visibility() {
        let _dir = TestConfigDir::new();
        write_file(
            r#"{"models":[{"id":"a","url":"https://a"}],"availableModels":["a","foreign"]}"#,
        );

        let removed = remove_model("a").unwrap().expect("removed model");
        assert!(removed.was_available);
        assert!(read_snapshot().unwrap().models.is_empty());
        assert_eq!(
            read_available_models().unwrap(),
            Some(vec!["foreign".to_string()])
        );

        // Restoring puts the entry and its visibility back.
        restore_model("a", &removed.config, removed.was_available).unwrap();
        assert_eq!(
            read_snapshot().unwrap().models.keys().collect::<Vec<_>>(),
            ["a"]
        );
        assert!(is_available(
            read_available_models().unwrap().as_deref(),
            "a"
        ));
    }

    #[test]
    #[serial]
    fn every_write_keeps_a_restorable_backup() {
        let _dir = TestConfigDir::new();
        write_file(r#"{"models":[{"id":"a","url":"https://a"}],"availableModels":["a"]}"#);
        let before = fs::read_to_string(get_models_path().unwrap()).unwrap();

        insert_model("b", &model("b")).unwrap();
        assert!(backup_exists().unwrap());

        assert!(restore_backup().unwrap());
        assert_eq!(
            fs::read_to_string(get_models_path().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            read_snapshot().unwrap().models.keys().collect::<Vec<_>>(),
            ["a"]
        );
    }

    #[test]
    #[serial]
    fn restore_without_a_backup_is_a_no_op() {
        let _dir = TestConfigDir::new();
        assert!(!restore_backup().unwrap());
        assert!(!backup_exists().unwrap());
    }

    /// 恢复方向没有 revision 锚点，唯一能保住用户改动的办法是先把当前状态也备份一份。
    /// 所以恢复两次应该在「恢复前」和「恢复后」之间来回，而不是把中间状态吃掉。
    #[test]
    #[serial]
    fn restore_keeps_the_state_it_replaces() {
        let _dir = TestConfigDir::new();
        write_file(r#"{"models":[{"id":"a","url":"https://a"}]}"#);
        let original = fs::read_to_string(get_models_path().unwrap()).unwrap();

        insert_model("b", &model("b")).unwrap();
        let after_insert = fs::read_to_string(get_models_path().unwrap()).unwrap();
        assert_ne!(original, after_insert);

        assert!(restore_backup().unwrap());
        assert_eq!(
            fs::read_to_string(get_models_path().unwrap()).unwrap(),
            original
        );

        // 恢复前的状态成了最新备份，再恢复一次就回到它。
        assert!(restore_backup().unwrap());
        assert_eq!(
            fs::read_to_string(get_models_path().unwrap()).unwrap(),
            after_insert
        );
    }

    #[test]
    #[serial]
    fn retention_keeps_only_the_newest_backups_and_spares_other_apps() {
        let _dir = TestConfigDir::new();
        let dir = backup_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        for stamp in [
            "20260101_000000",
            "20260102_000000",
            "20260103_000000",
            "20260104_000000",
        ] {
            fs::write(
                dir.join(format!("{BACKUP_PREFIX}{stamp}.json")),
                r#"{"models":[]}"#,
            )
            .unwrap();
        }
        // 别的应用的备份躺在同一个 backups 目录下，清理时不能顺手带走。
        let foreign = dir.join("pi_20260101_000000.json");
        fs::write(&foreign, r#"{"models":[]}"#).unwrap();

        prune_backups_to(2).unwrap();

        let mut names: Vec<String> = fs::read_dir(&dir)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        assert_eq!(
            names,
            [
                "pi_20260101_000000.json",
                "workbuddy_20260103_000000.json",
                "workbuddy_20260104_000000.json",
            ]
        );
    }

    #[test]
    #[serial]
    fn a_broken_backup_is_refused_and_the_file_is_left_alone() {
        let _dir = TestConfigDir::new();
        write_file(r#"{"models":[{"id":"a","url":"https://a"}]}"#);
        let before = fs::read_to_string(get_models_path().unwrap()).unwrap();

        let dir = backup_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join(format!("{BACKUP_PREFIX}20200101_000000.json")),
            "{ not json",
        )
        .unwrap();

        let error = restore_backup().expect_err("broken backup must be refused");
        assert!(error.to_string().contains("not valid JSON"));
        assert_eq!(
            fs::read_to_string(get_models_path().unwrap()).unwrap(),
            before
        );
    }

    #[test]
    #[serial]
    fn a_backup_with_the_wrong_shape_is_refused() {
        let _dir = TestConfigDir::new();
        write_file(r#"{"models":[{"id":"a","url":"https://a"}]}"#);
        let before = fs::read_to_string(get_models_path().unwrap()).unwrap();

        let dir = backup_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        let backup = dir.join(format!("{BACKUP_PREFIX}20200101_000000.json"));
        // 合法 JSON 但结构坏了：WorkBuddy 读不了，恢复也会把它照抄回去。
        fs::write(&backup, r#"{"models":"oops"}"#).unwrap();

        let error = restore_backup().expect_err("wrong shape must be refused");
        assert!(error.to_string().contains("must be an array"));
        assert_eq!(
            fs::read_to_string(get_models_path().unwrap()).unwrap(),
            before
        );

        fs::write(&backup, r#"{"models":[],"availableModels":[1]}"#).unwrap();
        let error = restore_backup().expect_err("bad visibility list must be refused");
        assert!(error.to_string().contains("must be a string"));
        assert_eq!(
            fs::read_to_string(get_models_path().unwrap()).unwrap(),
            before
        );
    }

    #[test]
    #[serial]
    fn a_failed_restore_reports_the_error_and_keeps_the_backup() {
        let _dir = TestConfigDir::new();
        let dir = backup_dir().unwrap();
        fs::create_dir_all(&dir).unwrap();
        let backup = dir.join(format!("{BACKUP_PREFIX}20200101_000000.json"));
        fs::write(&backup, r#"{"models":[]}"#).unwrap();

        // models.json 变成了目录：读不成也写不成，恢复必须报错而不是半途而废。
        let path = get_models_path().unwrap();
        fs::create_dir_all(&path).unwrap();

        assert!(restore_backup().is_err());
        assert!(path.is_dir());
        assert!(backup.exists());
    }

    #[test]
    #[serial]
    fn wrong_root_shapes_are_rejected() {
        let _dir = TestConfigDir::new();
        write_file("[]");
        assert!(read_snapshot()
            .unwrap_err()
            .to_string()
            .contains("root must be an object"));

        write_file(r#"{"models":{}}"#);
        assert!(read_snapshot()
            .unwrap_err()
            .to_string()
            .contains("must be an array"));

        write_file(r#"{"models":[{"url":"https://a"}]}"#);
        assert!(read_snapshot()
            .unwrap_err()
            .to_string()
            .contains("no string 'id'"));

        write_file(r#"{"models":[],"availableModels":[1]}"#);
        assert!(read_available_models()
            .unwrap_err()
            .to_string()
            .contains("must be a string"));
    }

    #[test]
    #[serial]
    fn model_credentials_are_read_from_the_native_fields() {
        let _dir = TestConfigDir::new();
        insert_model("a", &model("a")).unwrap();
        let saved = read_model("a").unwrap().unwrap();
        assert_eq!(
            model_base_url(&saved).unwrap(),
            "https://api.example.com/v1/chat/completions"
        );
        assert_eq!(model_api_key(&saved), "test-key");
        assert!(model_base_url(&json!({"id": "a"})).is_err());
    }

    #[cfg(unix)]
    #[test]
    #[serial]
    fn newly_created_file_and_directory_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let _dir = TestConfigDir::new();
        insert_model("a", &model("a")).unwrap();

        let path = get_models_path().unwrap();
        let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(file_mode, 0o600);
        assert_eq!(dir_mode, 0o700);
    }
}
