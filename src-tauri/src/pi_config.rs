//! Pi Agent live configuration helpers.
//!
//! Pi keeps model providers in `models.json` and the active provider/model in
//! `settings.json` under `~/.pi/agent` by default.

use serde_json::{json, Map, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use crate::config::{atomic_write, get_home_dir, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;

#[derive(Debug)]
struct JsonFileSnapshot {
    path: PathBuf,
    raw: Option<Vec<u8>>,
    value: Value,
}

#[derive(Clone, Copy)]
struct PendingDocument<'a> {
    snapshot: &'a JsonFileSnapshot,
    next: &'a Value,
}

fn pi_config_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn lock_pi_config() -> Result<std::sync::MutexGuard<'static, ()>, AppError> {
    pi_config_lock()
        .lock()
        .map_err(|_| AppError::Message("Pi configuration write lock is poisoned".to_string()))
}

pub fn get_pi_dir() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_pi_override_dir() {
        return override_dir;
    }

    if let Some(raw) = std::env::var_os("PI_CODING_AGENT_DIR") {
        let value = raw.to_string_lossy();
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed);
        }
    }

    get_home_dir().join(".pi").join("agent")
}

pub fn get_pi_models_path() -> PathBuf {
    get_pi_dir().join("models.json")
}

pub fn get_pi_settings_path() -> PathBuf {
    get_pi_dir().join("settings.json")
}

fn read_snapshot(path: &Path) -> Result<JsonFileSnapshot, AppError> {
    let raw = match fs::read(path) {
        Ok(raw) => Some(raw),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(AppError::io(path, error)),
    };
    let value = match raw.as_deref() {
        None => Value::Object(Map::new()),
        Some(raw) if raw.iter().all(|byte| byte.is_ascii_whitespace()) => Value::Object(Map::new()),
        Some(raw) => serde_json::from_slice(raw).map_err(|error| AppError::json(path, error))?,
    };

    Ok(JsonFileSnapshot {
        path: path.to_path_buf(),
        raw,
        value,
    })
}

fn read_json_or_empty_object(path: &Path) -> Result<Value, AppError> {
    read_snapshot(path).map(|snapshot| snapshot.value)
}

fn current_raw(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::read(path) {
        Ok(raw) => Ok(Some(raw)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn ensure_unchanged(snapshot: &JsonFileSnapshot) -> Result<(), AppError> {
    if current_raw(&snapshot.path)? != snapshot.raw {
        return Err(AppError::Config(format!(
            "Pi configuration changed on disk while CC Switch was updating it: {}. Reload and try again.",
            snapshot.path.display()
        )));
    }
    Ok(())
}

fn restore_snapshot(snapshot: &JsonFileSnapshot) -> Result<(), AppError> {
    match &snapshot.raw {
        Some(raw) => atomic_write(&snapshot.path, raw),
        None if snapshot.path.exists() => {
            fs::remove_file(&snapshot.path).map_err(|error| AppError::io(&snapshot.path, error))
        }
        None => Ok(()),
    }
}

fn apply_documents_with<F>(documents: &[PendingDocument<'_>], mut writer: F) -> Result<(), AppError>
where
    F: FnMut(&Path, &Value) -> Result<(), AppError>,
{
    for (index, document) in documents.iter().enumerate() {
        if let Err(operation_error) = writer(&document.snapshot.path, document.next) {
            let rollback_errors = documents[..=index]
                .iter()
                .rev()
                .filter_map(|document| {
                    restore_snapshot(document.snapshot)
                        .err()
                        .map(|error| format!("{}: {error}", document.snapshot.path.display()))
                })
                .collect::<Vec<_>>();
            return if rollback_errors.is_empty() {
                Err(operation_error)
            } else {
                Err(AppError::Message(format!(
                    "{operation_error}; rollback also failed for {}",
                    rollback_errors.join(", ")
                )))
            };
        }
    }
    Ok(())
}

fn commit_documents(
    models_snapshot: &JsonFileSnapshot,
    models_next: &Value,
    settings: Option<(&JsonFileSnapshot, &Value)>,
) -> Result<(), AppError> {
    let models_changed = models_next != &models_snapshot.value;
    let settings_changed = settings.is_some_and(|(snapshot, next)| next != &snapshot.value);
    if !models_changed && !settings_changed {
        return Ok(());
    }

    // Every loaded document is an observed dependency. For example, removal
    // reads settings.json even though it only writes models.json.
    ensure_unchanged(models_snapshot)?;
    if let Some((settings_snapshot, _)) = settings {
        ensure_unchanged(settings_snapshot)?;
    }

    let mut documents = Vec::with_capacity(2);
    if models_changed {
        documents.push(PendingDocument {
            snapshot: models_snapshot,
            next: models_next,
        });
    }
    if let Some((settings_snapshot, settings_next)) = settings {
        if settings_changed {
            documents.push(PendingDocument {
                snapshot: settings_snapshot,
                next: settings_next,
            });
        }
    }

    apply_documents_with(&documents, write_json_file)
}

fn mutate_pi_documents<F>(include_settings: bool, mutate: F) -> Result<(), AppError>
where
    F: FnOnce(&mut Value, Option<&mut Value>) -> Result<(), AppError>,
{
    let _guard = lock_pi_config()?;
    let models_snapshot = read_snapshot(&get_pi_models_path())?;
    let settings_snapshot = include_settings
        .then(|| read_snapshot(&get_pi_settings_path()))
        .transpose()?;
    let mut models_next = models_snapshot.value.clone();
    let mut settings_next = settings_snapshot
        .as_ref()
        .map(|snapshot| snapshot.value.clone());

    mutate(&mut models_next, settings_next.as_mut())?;
    commit_documents(
        &models_snapshot,
        &models_next,
        settings_snapshot.as_ref().zip(settings_next.as_ref()),
    )
}

fn object_mut<'a>(
    value: &'a mut Value,
    path: &Path,
    description: &str,
) -> Result<&'a mut Map<String, Value>, AppError> {
    value.as_object_mut().ok_or_else(|| {
        AppError::Config(format!(
            "{description} must be a JSON object: {}",
            path.display()
        ))
    })
}

fn first_string<'a>(object: &'a Map<String, Value>, keys: &[&str]) -> Option<&'a str> {
    keys.iter()
        .find_map(|key| object.get(*key).and_then(Value::as_str))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn model_ids_from_config(config: &Value) -> Vec<String> {
    config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| match model {
                    Value::String(id) => Some(id.as_str()),
                    Value::Object(obj) => obj.get("id").and_then(Value::as_str),
                    _ => None,
                })
                .map(str::trim)
                .filter(|id| !id.is_empty())
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn normalized_models_from_config(config: &Value) -> Option<Vec<Value>> {
    config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| match model {
                    Value::String(id) => {
                        let id = id.trim();
                        (!id.is_empty()).then(|| Value::String(id.to_string()))
                    }
                    Value::Object(object) => {
                        let id = object.get("id").and_then(Value::as_str)?.trim();
                        if id.is_empty() {
                            return None;
                        }

                        let mut normalized = object.clone();
                        normalized.insert("id".to_string(), Value::String(id.to_string()));
                        Some(Value::Object(normalized))
                    }
                    _ => None,
                })
                .collect()
        })
}

fn normalize_provider_config(config: &Value) -> Result<Value, AppError> {
    let Some(source) = config.as_object() else {
        return Err(AppError::Config(
            "Pi provider settings must be a JSON object".to_string(),
        ));
    };

    let mut output = source.clone();

    if let Some(base_url) = first_string(source, &["baseUrl", "baseURL"]) {
        output.insert("baseUrl".to_string(), Value::String(base_url.to_string()));
        output.remove("baseURL");
    }

    if let Some(models) = normalized_models_from_config(config) {
        output.insert("models".to_string(), Value::Array(models));
    }

    // Pi stores the selected model in settings.json, not per provider.
    output.remove("defaultModel");

    Ok(Value::Object(output))
}

fn default_model_for_provider(config: &Value) -> Option<String> {
    config
        .get("defaultModel")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| model_ids_from_config(config).into_iter().next())
}

fn providers_mut<'a>(
    models_root: &'a mut Value,
    models_path: &Path,
) -> Result<&'a mut Map<String, Value>, AppError> {
    let root = object_mut(models_root, models_path, "Pi models.json root")?;
    let providers = root
        .entry("providers".to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    object_mut(providers, models_path, "Pi models.json providers")
}

fn insert_provider(
    models_root: &mut Value,
    models_path: &Path,
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    let providers = providers_mut(models_root, models_path)?;
    if providers.contains_key(&provider.id) && !overwrite_existing {
        return Err(AppError::Config(format!(
            "Pi provider '{}' already exists in models.json and is not managed by CC Switch",
            provider.id
        )));
    }

    providers.insert(
        provider.id.clone(),
        normalize_provider_config(&provider.settings_config)?,
    );
    Ok(())
}

fn select_provider(settings_root: &mut Value, provider: &Provider) -> Result<(), AppError> {
    let settings_path = get_pi_settings_path();
    let settings = object_mut(settings_root, &settings_path, "Pi settings.json root")?;
    settings.insert(
        "defaultProvider".to_string(),
        Value::String(provider.id.clone()),
    );
    if let Some(default_model) = default_model_for_provider(&provider.settings_config) {
        settings.insert("defaultModel".to_string(), Value::String(default_model));
    } else {
        settings.remove("defaultModel");
    }
    Ok(())
}

/// Add or update a provider in Pi's additive registry without selecting it.
pub fn upsert_pi_live_provider(
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    mutate_pi_documents(false, |models_root, _| {
        insert_provider(
            models_root,
            &get_pi_models_path(),
            provider,
            overwrite_existing,
        )
    })
}

/// Add or update a provider and select it as Pi's default provider/model.
pub fn activate_pi_live_provider(
    provider: &Provider,
    overwrite_existing: bool,
) -> Result<(), AppError> {
    mutate_pi_documents(true, |models_root, settings_root| {
        insert_provider(
            models_root,
            &get_pi_models_path(),
            provider,
            overwrite_existing,
        )?;
        select_provider(
            settings_root.expect("settings requested for Pi provider activation"),
            provider,
        )
    })
}

/// Compatibility wrapper retained for callers that mean "activate".
pub fn write_pi_live_provider(provider: &Provider) -> Result<(), AppError> {
    activate_pi_live_provider(provider, true)
}

/// Synchronize all CC Switch-managed providers and the selected default.
pub fn sync_pi_live_providers(
    providers: &[&Provider],
    active_provider: Option<&Provider>,
) -> Result<(), AppError> {
    mutate_pi_documents(active_provider.is_some(), |models_root, settings_root| {
        let models_path = get_pi_models_path();
        for provider in providers {
            insert_provider(models_root, &models_path, provider, true)?;
        }
        if let Some(active_provider) = active_provider {
            select_provider(
                settings_root.expect("settings requested for Pi provider sync"),
                active_provider,
            )?;
        }
        Ok(())
    })
}

pub fn pi_provider_exists(provider_id: &str) -> Result<bool, AppError> {
    let _guard = lock_pi_config()?;
    let models_path = get_pi_models_path();
    let models_root = read_json_or_empty_object(&models_path)?;
    let root = models_root.as_object().ok_or_else(|| {
        AppError::Config(format!(
            "Pi models.json root must be a JSON object: {}",
            models_path.display()
        ))
    })?;
    Ok(root
        .get("providers")
        .and_then(Value::as_object)
        .is_some_and(|providers| providers.contains_key(provider_id)))
}

/// Remove a managed provider, but never leave Pi's default pointing at a
/// provider that no longer exists.
pub fn remove_pi_live_provider(provider_id: &str) -> Result<(), AppError> {
    mutate_pi_documents(true, |models_root, settings_root| {
        let settings_root = settings_root.expect("settings requested for Pi provider removal");
        if settings_root.get("defaultProvider").and_then(Value::as_str) == Some(provider_id) {
            return Err(AppError::Config(format!(
                "Cannot remove active Pi provider '{provider_id}'. Switch first."
            )));
        }

        providers_mut(models_root, &get_pi_models_path())?.remove(provider_id);
        Ok(())
    })
}

fn provider_models_for_form(provider_config: &Value) -> Value {
    let models = provider_config
        .get("models")
        .and_then(Value::as_array)
        .map(|models| {
            models
                .iter()
                .filter_map(|model| match model {
                    Value::String(id) => Some(json!({ "id": id })),
                    Value::Object(obj) => Some(Value::Object(obj.clone())),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    Value::Array(models)
}

pub fn read_pi_live_settings() -> Result<Value, AppError> {
    let _guard = lock_pi_config()?;
    let models_path = get_pi_models_path();
    let settings_path = get_pi_settings_path();
    let models_root = read_json_or_empty_object(&models_path)?;
    let settings_root = read_json_or_empty_object(&settings_path)?;

    let default_provider = settings_root
        .get("defaultProvider")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if default_provider.is_empty() {
        return Err(AppError::Config(
            "Pi settings.json does not define defaultProvider".to_string(),
        ));
    }

    let provider_config = models_root
        .get("providers")
        .and_then(Value::as_object)
        .and_then(|providers| providers.get(default_provider))
        .ok_or_else(|| {
            AppError::Config(format!(
                "Pi models.json does not define provider '{default_provider}'"
            ))
        })?;

    let mut form_config = provider_config.clone();
    if let Some(obj) = form_config.as_object_mut() {
        if let Some(base_url) = obj.get("baseURL").cloned() {
            obj.insert("baseUrl".to_string(), base_url);
        }
        obj.insert(
            "models".to_string(),
            provider_models_for_form(provider_config),
        );
    }

    Ok(json!({
        "defaultProvider": default_provider,
        "defaultModel": settings_root.get("defaultModel").cloned().unwrap_or(Value::Null),
        "providerConfig": form_config
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn normalize_provider_config_writes_pi_base_url_key() {
        let config = json!({
            "baseURL": "https://api.example.com/v1",
            "apiKey": "sk-test",
            "api": "openai-completions",
            "models": []
        });

        let normalized = normalize_provider_config(&config).unwrap();

        assert_eq!(
            normalized.get("baseUrl"),
            Some(&json!("https://api.example.com/v1"))
        );
        assert!(
            normalized.get("baseURL").is_none(),
            "Pi models.json must use baseUrl, not baseURL"
        );
    }

    #[test]
    fn detects_external_changes_after_snapshot() {
        let dir = tempdir().expect("create temp dir");
        let path = dir.path().join("models.json");
        fs::write(&path, br#"{"providers":{}}"#).expect("seed models");
        let snapshot = read_snapshot(&path).expect("snapshot models");

        fs::write(&path, br#"{"providers":{"external":{}}}"#).expect("edit models");
        let error = ensure_unchanged(&snapshot).expect_err("external edit must be detected");

        assert!(error.to_string().contains("changed on disk"));
    }

    #[test]
    fn rolls_back_models_when_settings_write_fails() {
        let dir = tempdir().expect("create temp dir");
        let models_path = dir.path().join("models.json");
        let settings_path = dir.path().join("settings.json");
        let original_models = br#"{"providers":{"old":{}}}"#;
        let original_settings = br#"{"defaultProvider":"old"}"#;
        fs::write(&models_path, original_models).expect("seed models");
        fs::write(&settings_path, original_settings).expect("seed settings");
        let models_snapshot = read_snapshot(&models_path).expect("snapshot models");
        let settings_snapshot = read_snapshot(&settings_path).expect("snapshot settings");
        let models_next = json!({"providers": {"old": {}, "new": {}}});
        let settings_next = json!({"defaultProvider": "new"});
        let documents = [
            PendingDocument {
                snapshot: &models_snapshot,
                next: &models_next,
            },
            PendingDocument {
                snapshot: &settings_snapshot,
                next: &settings_next,
            },
        ];

        let error = apply_documents_with(&documents, |path, value| {
            if path == settings_path {
                return Err(AppError::Message(
                    "injected settings write failure".to_string(),
                ));
            }
            write_json_file(path, value)
        })
        .expect_err("settings write should fail");

        assert!(error
            .to_string()
            .contains("injected settings write failure"));
        assert_eq!(fs::read(&models_path).unwrap(), original_models);
        assert_eq!(fs::read(&settings_path).unwrap(), original_settings);
    }
}
