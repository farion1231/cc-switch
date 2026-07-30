//! Pi Agent live configuration helpers.
//!
//! Pi keeps model providers in `models.json` and the active provider/model in
//! `settings.json` under `~/.pi/agent` by default.

use serde_json::{json, Map, Value};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::{atomic_write, get_app_config_dir, get_home_dir, write_json_file};
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

fn cleanup_backups(backup_root: &Path) -> Result<(), AppError> {
    let retain = crate::settings::effective_backup_retain_count();
    let mut entries = fs::read_dir(backup_root)
        .map_err(|error| AppError::io(backup_root, error))?
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .collect::<Vec<_>>();
    if entries.len() <= retain {
        return Ok(());
    }

    entries.sort_by_key(|entry| entry.metadata().and_then(|meta| meta.modified()).ok());
    let remove_count = entries.len().saturating_sub(retain);
    for entry in entries.into_iter().take(remove_count) {
        if let Err(error) = fs::remove_dir_all(entry.path()) {
            log::warn!(
                "Failed to remove old Pi backup {}: {error}",
                entry.path().display()
            );
        }
    }
    Ok(())
}

fn create_backup(snapshots: &[&JsonFileSnapshot]) -> Result<(), AppError> {
    if snapshots.iter().all(|snapshot| snapshot.raw.is_none()) {
        return Ok(());
    }

    let backup_root = get_app_config_dir().join("backups").join("pi");
    fs::create_dir_all(&backup_root).map_err(|error| AppError::io(&backup_root, error))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let base_name = format!("pi_{timestamp}");
    let mut backup_dir = backup_root.join(&base_name);
    let mut suffix = 1;
    while backup_dir.exists() {
        backup_dir = backup_root.join(format!("{base_name}_{suffix}"));
        suffix += 1;
    }
    fs::create_dir_all(&backup_dir).map_err(|error| AppError::io(&backup_dir, error))?;

    for snapshot in snapshots {
        let Some(raw) = snapshot.raw.as_deref() else {
            continue;
        };
        let filename = snapshot
            .path
            .file_name()
            .ok_or_else(|| AppError::Config("Invalid Pi config filename".to_string()))?;
        let backup_path = backup_dir.join(filename);
        atomic_write(&backup_path, raw)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&backup_path, fs::Permissions::from_mode(0o600))
                .map_err(|error| AppError::io(&backup_path, error))?;
        }
    }
    cleanup_backups(&backup_root)
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

    let snapshots = documents
        .iter()
        .map(|document| document.snapshot)
        .collect::<Vec<_>>();
    create_backup(&snapshots)?;
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

const PI_SUPPORTED_APIS: [&str; 4] = [
    "openai-completions",
    "openai-responses",
    "anthropic-messages",
    "google-generative-ai",
];

fn validate_pi_api(value: Option<&Value>, field: &str) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let api = value
        .as_str()
        .ok_or_else(|| AppError::Config(format!("Pi provider field '{field}' must be a string")))?;
    if !PI_SUPPORTED_APIS.contains(&api) {
        return Err(AppError::Config(format!(
            "Pi provider field '{field}' uses unsupported API '{api}'"
        )));
    }
    Ok(())
}

fn validate_pi_headers(value: Option<&Value>, field: &str) -> Result<(), AppError> {
    let Some(value) = value else {
        return Ok(());
    };
    let headers = value.as_object().ok_or_else(|| {
        AppError::Config(format!("Pi provider field '{field}' must be an object"))
    })?;
    if let Some((name, _)) = headers
        .iter()
        .find(|(name, value)| name.trim().is_empty() || !value.is_string())
    {
        return Err(AppError::Config(format!(
            "Pi provider header '{field}.{name}' must have a non-empty name and string value"
        )));
    }
    Ok(())
}

/// Validate the Pi fields CC Switch understands while allowing unknown fields
/// to pass through for forward compatibility with newer Pi releases.
pub(crate) fn validate_pi_provider_config(config: &Value) -> Result<(), AppError> {
    let source = config.as_object().ok_or_else(|| {
        AppError::Config("Pi provider settings must be a JSON object".to_string())
    })?;

    let mut base_urls = Vec::with_capacity(2);
    for field in ["baseUrl", "baseURL"] {
        if let Some(value) = source.get(field) {
            let base_url = value.as_str().ok_or_else(|| {
                AppError::Config(format!("Pi provider field '{field}' must be a string"))
            })?;
            let base_url = base_url.trim();
            if base_url.is_empty() {
                return Err(AppError::Config(format!(
                    "Pi provider field '{field}' must not be empty"
                )));
            }
            base_urls.push((field, base_url));
        }
    }
    if base_urls.len() == 2 && base_urls[0].1 != base_urls[1].1 {
        return Err(AppError::Config(
            "Pi provider fields 'baseUrl' and 'baseURL' must not conflict".to_string(),
        ));
    }

    validate_pi_api(source.get("api"), "api")?;
    if source.get("apiKey").is_some_and(|value| !value.is_string()) {
        return Err(AppError::Config(
            "Pi provider field 'apiKey' must be a string".to_string(),
        ));
    }
    if let Some(oauth) = source.get("oauth") {
        if oauth.as_str() != Some("radius") {
            return Err(AppError::Config(
                "Pi provider field 'oauth' currently supports only 'radius'".to_string(),
            ));
        }
        if base_urls.is_empty() {
            return Err(AppError::Config(
                "Pi provider field 'baseUrl' is required when 'oauth' is configured".to_string(),
            ));
        }
    }
    if source
        .get("authHeader")
        .is_some_and(|value| !value.is_boolean())
    {
        return Err(AppError::Config(
            "Pi provider field 'authHeader' must be a boolean".to_string(),
        ));
    }
    validate_pi_headers(source.get("headers"), "headers")?;
    for field in ["compat", "modelOverrides"] {
        if source.get(field).is_some_and(|value| !value.is_object()) {
            return Err(AppError::Config(format!(
                "Pi provider field '{field}' must be an object"
            )));
        }
    }

    if source
        .get("defaultModel")
        .is_some_and(|value| !value.is_string())
    {
        return Err(AppError::Config(
            "Pi provider field 'defaultModel' must be a string".to_string(),
        ));
    }
    let Some(models_value) = source.get("models") else {
        return Ok(());
    };
    let models = models_value.as_array().ok_or_else(|| {
        AppError::Config("Pi provider field 'models' must be an array".to_string())
    })?;

    // Whether baseUrl/api/defaultModel may be inherited depends on Pi's
    // evolving built-in provider catalog, so this fragment validator must not
    // hard-code those contextual requirements.
    let mut model_ids = HashSet::with_capacity(models.len());
    for (index, model) in models.iter().enumerate() {
        let id = match model {
            // Accept the legacy shorthand already supported by CC Switch and
            // normalize it during writes.
            Value::String(id) => id.trim(),
            Value::Object(model) => {
                let id = model.get("id").and_then(Value::as_str).ok_or_else(|| {
                    AppError::Config(format!(
                        "Pi provider field 'models[{index}].id' must be a string"
                    ))
                })?;
                validate_pi_api(model.get("api"), &format!("models[{index}].api"))?;
                id.trim()
            }
            _ => {
                return Err(AppError::Config(format!(
                    "Pi provider field 'models[{index}]' must be an object or string"
                )));
            }
        };
        if id.is_empty() {
            return Err(AppError::Config(format!(
                "Pi provider field 'models[{index}].id' must not be empty"
            )));
        }
        if !model_ids.insert(id) {
            return Err(AppError::Config(format!(
                "Pi provider contains duplicate model id '{id}'"
            )));
        }
    }

    Ok(())
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
    validate_pi_provider_config(config)?;

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
            "models": [],
            "futurePiField": { "keep": true }
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
        assert_eq!(
            normalized.pointer("/futurePiField/keep"),
            Some(&json!(true)),
            "unknown Pi fields must survive normalization"
        );
    }

    #[test]
    fn validates_all_official_pi_apis_without_requiring_an_api_key() {
        for api in PI_SUPPORTED_APIS {
            validate_pi_provider_config(&json!({
                "baseUrl": "https://api.example.com/v1",
                "api": api,
                "models": [{ "id": "model-1" }]
            }))
            .unwrap_or_else(|error| panic!("{api} should be accepted: {error}"));
        }

        validate_pi_provider_config(&json!({
            "baseUrl": "https://api.example.com/v1",
            "models": [{ "id": "model-1", "api": "anthropic-messages" }]
        }))
        .expect("model-level API should satisfy Pi's API requirement");
    }

    #[test]
    fn allows_builtin_overrides_and_future_fields() {
        validate_pi_provider_config(&json!({
            "baseUrl": "https://proxy.example.com/v1",
            "oauth": "radius",
            "authHeader": true,
            "headers": { "x-route": "$PI_ROUTE" },
            "compat": { "supportsDeveloperRole": false },
            "modelOverrides": {
                "known-model": { "contextWindow": 200000 }
            },
            "futurePiField": { "enabled": true }
        }))
        .expect("built-in overrides and unknown future fields should be accepted");

        validate_pi_provider_config(&json!({
            "models": [{ "id": "custom-model" }],
            "defaultModel": "built-in-model"
        }))
        .expect("built-in providers may inherit endpoint, API, and model catalog entries");
    }

    #[test]
    fn rejects_invalid_pi_provider_schema() {
        let cases = [
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-chat",
                    "models": [{ "id": "model-1" }]
                }),
                "unsupported API",
            ),
            (
                json!({
                    "baseUrl": "",
                    "api": "openai-completions"
                }),
                "baseUrl",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "models": [{ "name": "missing-id" }]
                }),
                "models[0].id",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-completions",
                    "models": [{ "id": "same" }, { "id": "same" }]
                }),
                "duplicate model id",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-completions",
                    "models": [{ "id": "model-1" }],
                    "defaultModel": 42
                }),
                "defaultModel",
            ),
            (
                json!({
                    "baseUrl": "https://api.example.com/v1",
                    "api": "openai-completions",
                    "headers": { "x-route": 42 },
                    "models": [{ "id": "model-1" }]
                }),
                "string value",
            ),
        ];

        for (config, expected) in cases {
            let error =
                validate_pi_provider_config(&config).expect_err("invalid config must be rejected");
            assert!(
                error.to_string().contains(expected),
                "expected '{expected}' in error, got {error}"
            );
        }
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
