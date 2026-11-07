//! Qwen Code keeps every native provider in one JSONC `settings.json`:
//! `modelProviders.<authType>[]` holds model entries, each naming the environment
//! variable (`envKey`) that carries its API key, and `env` may hold that key.
//!
//! A saved CCS provider is `{ authType, baseUrl, apiKey, envKey?, models: [...] }`.
//! Entries are grouped into providers by `envKey`: CCS-created providers own
//! `CC_SWITCH_QWEN_<KEY>`, so the provider key and the variable map one to one;
//! other variables (e.g. `OPENAI_API_KEY`) keep their name in `envKey`.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use crate::jsonc_document::JsoncDocument;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) const MANAGED_ENV_PREFIX: &str = "CC_SWITCH_QWEN_";
pub(crate) const AUTH_TYPES: [&str; 5] = [
    "openai",
    "openai-responses",
    "anthropic",
    "gemini",
    "vertex-ai",
];
/// Qwen Code's current settings schema version (`SETTINGS_VERSION`).
const SETTINGS_VERSION: u64 = 4;

/// `QWEN_HOME`, else `~/.qwen`. Qwen expands a leading `~/` itself.
pub(crate) fn data_dir() -> PathBuf {
    match std::env::var("QWEN_HOME") {
        Ok(value) if !value.trim().is_empty() => expand_home(value.trim()),
        _ => get_home_dir().join(".qwen"),
    }
}

fn expand_home(value: &str) -> PathBuf {
    if value == "~" {
        return get_home_dir();
    }
    match value
        .strip_prefix("~/")
        .or_else(|| value.strip_prefix("~\\"))
    {
        Some(rest) => get_home_dir().join(rest),
        None => PathBuf::from(value),
    }
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("settings.json")
}

/// Projects and chats live under `QWEN_RUNTIME_DIR`, then
/// `advanced.runtimeOutputDir`, then the config home.
pub(crate) fn runtime_dir() -> PathBuf {
    if let Ok(value) = std::env::var("QWEN_RUNTIME_DIR") {
        if !value.trim().is_empty() {
            return expand_home(value.trim());
        }
    }
    read(&config_path())
        .ok()
        .and_then(|settings| {
            settings
                .pointer("/advanced/runtimeOutputDir")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|dir| !dir.is_empty())
                .map(expand_home)
        })
        .unwrap_or_else(data_dir)
}

pub(crate) fn lock() -> Result<MutexGuard<'static, ()>, AppError> {
    WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))
}

pub(crate) fn managed_env_key(id: &str) -> String {
    format!(
        "{MANAGED_ENV_PREFIX}{}",
        id.to_ascii_uppercase().replace('-', "_")
    )
}

fn is_provider_key(id: &str) -> bool {
    !id.is_empty()
        && id.split('-').all(|part| {
            !part.is_empty() && part.bytes().all(|b| matches!(b, b'a'..=b'z' | b'0'..=b'9'))
        })
}

/// Inverse of [`managed_env_key`] for our variables; a readable key otherwise.
fn provider_id_for_env_key(env_key: &str) -> String {
    if let Some(suffix) = env_key.strip_prefix(MANAGED_ENV_PREFIX) {
        let id = suffix.to_ascii_lowercase().replace('_', "-");
        if is_provider_key(&id) {
            return id;
        }
    }
    let mut id = String::new();
    for c in env_key.chars() {
        if c.is_ascii_alphanumeric() {
            id.push(c.to_ascii_lowercase());
        } else if !id.is_empty() && !id.ends_with('-') {
            id.push('-');
        }
    }
    let id = id.trim_end_matches('-').to_string();
    if id.is_empty() {
        "env".into()
    } else {
        id
    }
}

/// The variable a provider's entries are written with.
pub(crate) fn env_key_for(id: &str, config: &Value) -> String {
    config
        .get("envKey")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| managed_env_key(id))
}

fn strip_bom(text: &str) -> &str {
    text.strip_prefix('\u{feff}').unwrap_or(text)
}

fn read_source(path: &Path) -> Result<Option<String>, AppError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn parse(text: &str) -> Result<Value, AppError> {
    let text = strip_bom(text);
    if text.trim().is_empty() {
        return Ok(json!({}));
    }
    let value = JsoncDocument::parse(text)
        .map_err(|_| AppError::Config("Invalid Qwen Code settings.json".into()))?
        .value()
        .clone();
    for key in ["modelProviders", "env", "mcpServers", "security", "model"] {
        if value.get(key).is_some_and(|v| !v.is_object()) {
            return Err(AppError::Config(format!(
                "Qwen Code settings.json '{key}' must be an object"
            )));
        }
    }
    Ok(value)
}

pub(crate) fn read(path: &Path) -> Result<Value, AppError> {
    match read_source(path)? {
        Some(text) => parse(&text),
        None => Ok(json!({})),
    }
}

pub(crate) fn read_settings() -> Result<Value, AppError> {
    read(&config_path())
}

/// Keep comments, formatting and every untouched key of the user's file.
fn write(path: &Path, after: &Value) -> Result<(), AppError> {
    let original = read_source(path)?.unwrap_or_default();
    let source = strip_bom(&original);
    let text = if source.trim().is_empty() {
        let mut fresh = Map::new();
        fresh.insert("$version".into(), json!(SETTINGS_VERSION));
        for (key, value) in after.as_object().into_iter().flatten() {
            fresh.insert(key.clone(), value.clone());
        }
        serde_json::to_string_pretty(&Value::Object(fresh))
            .map_err(|e| AppError::Config(e.to_string()))?
            + "\n"
    } else {
        let mut document = JsoncDocument::parse(source)
            .map_err(|_| AppError::Config("Invalid Qwen Code settings.json".into()))?;
        document.apply(after)?;
        document.validated_source()?
    };
    atomic_write_private(path, text.as_bytes())
}

/// Write the settings, run `commit`, and restore the previous file if it fails.
pub(crate) fn update_and_commit<T>(
    path: &Path,
    mutate: impl FnOnce(&Value) -> Result<Value, AppError>,
    commit: impl FnOnce(&Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let before = read(path)?;
    let after = mutate(&before)?;
    crate::mcode_config::write_and_commit(
        path,
        || {
            if before == after {
                Ok(())
            } else {
                write(path, &after)
            }
        },
        || commit(&after),
    )
}

struct Group {
    env_key: String,
    config: Value,
}

/// Group native entries into CCS providers, keyed by provider id.
fn groups(document: &Value) -> IndexMap<String, Group> {
    let mut by_env: IndexMap<String, (String, Vec<Value>)> = IndexMap::new();
    for (auth_type, entries) in document
        .get("modelProviders")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for entry in entries.as_array().into_iter().flatten() {
            let Some(env_key) = entry
                .get("envKey")
                .and_then(Value::as_str)
                .filter(|key| !key.trim().is_empty())
            else {
                continue;
            };
            if entry.get("id").and_then(Value::as_str).is_none() {
                continue;
            }
            let group = by_env
                .entry(env_key.to_string())
                .or_insert_with(|| (auth_type.clone(), Vec::new()));
            let mut model = entry.clone();
            let object = model.as_object_mut().unwrap();
            object.remove("envKey");
            if *auth_type != group.0 {
                object.insert("authType".into(), json!(auth_type));
            }
            group.1.push(model);
        }
    }

    // Managed variables map to their provider key one to one, so they claim
    // their ids first; readable ids of other variables yield on a collision.
    let mut ids: IndexMap<String, String> = IndexMap::new();
    for env_key in by_env.keys() {
        let id = provider_id_for_env_key(env_key);
        if managed_env_key(&id) == *env_key {
            ids.insert(env_key.clone(), id);
        }
    }
    for env_key in by_env.keys() {
        if ids.contains_key(env_key) {
            continue;
        }
        let base = provider_id_for_env_key(env_key);
        let mut id = base.clone();
        let mut n = 2;
        while ids.values().any(|taken| *taken == id) {
            id = format!("{base}-{n}");
            n += 1;
        }
        ids.insert(env_key.clone(), id);
    }

    let env = document.get("env").and_then(Value::as_object);
    let mut result: IndexMap<String, Group> = IndexMap::new();
    for (env_key, (auth_type, mut models)) in by_env {
        let base_url = models
            .first()
            .and_then(|model| model.get("baseUrl"))
            .and_then(Value::as_str)
            .map(str::to_string);
        for model in &mut models {
            if model.get("baseUrl").and_then(Value::as_str) == base_url.as_deref() {
                model.as_object_mut().unwrap().remove("baseUrl");
            }
        }
        let mut config = Map::new();
        config.insert("authType".into(), json!(auth_type));
        if let Some(base_url) = base_url {
            config.insert("baseUrl".into(), json!(base_url));
        }
        if let Some(api_key) = env
            .and_then(|env| env.get(&env_key))
            .and_then(Value::as_str)
        {
            config.insert("apiKey".into(), json!(api_key));
        }
        let id = ids[&env_key].clone();
        if managed_env_key(&id) != env_key {
            config.insert("envKey".into(), json!(env_key));
        }
        config.insert("models".into(), Value::Array(models));
        result.insert(
            id,
            Group {
                env_key,
                config: Value::Object(config),
            },
        );
    }
    result
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read(&config_path())?;
    Ok(groups(&document)
        .into_iter()
        .map(|(id, group)| (id, group.config))
        .collect())
}

pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    let document = read(&config_path())?;
    let groups = groups(&document);
    Ok(groups.contains_key(id)
        || groups
            .values()
            .any(|group| group.env_key == managed_env_key(id)))
}

fn auth_type_of<'a>(value: &'a Value, fallback: &'a str) -> &'a str {
    value
        .get("authType")
        .and_then(Value::as_str)
        .unwrap_or(fallback)
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() || id.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Enter a Qwen Code provider key".into(),
        ));
    }
    let Some(object) = config.as_object() else {
        return Err(AppError::InvalidInput(
            "Qwen Code provider configuration must be an object".into(),
        ));
    };
    if config.get("envKey").is_none() && !is_provider_key(id) {
        return Err(AppError::InvalidInput(format!(
            "Qwen Code provider key '{id}' may only contain lowercase letters, digits and single hyphens"
        )));
    }
    if let Some(env_key) = object.get("envKey") {
        let valid = env_key.as_str().is_some_and(|key| {
            let mut chars = key.chars();
            chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        if !valid {
            return Err(AppError::InvalidInput(
                "Qwen Code envKey must be a valid environment variable name".into(),
            ));
        }
    }
    let auth_type = object
        .get("authType")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !AUTH_TYPES.contains(&auth_type) {
        return Err(AppError::InvalidInput(format!(
            "Unsupported Qwen Code auth type '{auth_type}'"
        )));
    }
    for field in ["baseUrl", "apiKey"] {
        if object.get(field).is_some_and(|v| !v.is_string()) {
            return Err(AppError::InvalidInput(format!(
                "Qwen Code provider '{field}' must be a string"
            )));
        }
    }
    let base_url = object
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or_default();
    let models = object
        .get("models")
        .and_then(Value::as_array)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Add at least one Qwen Code model".into()))?;
    let mut seen = std::collections::HashSet::new();
    for model in models {
        let model_id = model
            .get("id")
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .ok_or_else(|| AppError::InvalidInput("Every Qwen Code model needs an ID".into()))?;
        let model_auth = auth_type_of(model, auth_type);
        if !AUTH_TYPES.contains(&model_auth) {
            return Err(AppError::InvalidInput(format!(
                "Unsupported Qwen Code auth type '{model_auth}'"
            )));
        }
        let model_base = model
            .get("baseUrl")
            .and_then(Value::as_str)
            .map(str::trim)
            .unwrap_or(base_url);
        if model_auth == "anthropic" && model_base.is_empty() {
            return Err(AppError::InvalidInput(
                "Anthropic-compatible Qwen Code providers need a base URL".into(),
            ));
        }
        if !seen.insert((
            model_auth.to_string(),
            model_id.to_string(),
            model_base.to_string(),
        )) {
            return Err(AppError::InvalidInput(format!(
                "Qwen Code model '{model_id}' is listed twice"
            )));
        }
        if let Some(window) = model.pointer("/generationConfig/contextWindowSize") {
            if window.as_u64().is_none_or(|n| n == 0) {
                return Err(AppError::InvalidInput(format!(
                    "Context window of Qwen Code model '{model_id}' must be a positive integer"
                )));
            }
        }
    }
    Ok(())
}

/// Native entries for one CCS provider, with their target `authType` array.
fn native_entries(config: &Value, env_key: &str) -> Vec<(String, Value)> {
    let auth_type = auth_type_of(config, "openai").to_string();
    let base_url = config
        .get("baseUrl")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|url| !url.is_empty());
    config
        .get("models")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|model| {
            let target = auth_type_of(model, &auth_type).to_string();
            let mut entry = Map::new();
            entry.insert(
                "id".into(),
                json!(model["id"].as_str().unwrap_or_default().trim()),
            );
            for key in ["name", "description"] {
                if let Some(value) = model.get(key) {
                    entry.insert(key.into(), value.clone());
                }
            }
            let model_base = model
                .get("baseUrl")
                .and_then(Value::as_str)
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .or(base_url);
            if let Some(url) = model_base {
                entry.insert("baseUrl".into(), json!(url));
            }
            entry.insert("envKey".into(), json!(env_key));
            for (key, value) in model.as_object().into_iter().flatten() {
                if !matches!(
                    key.as_str(),
                    "id" | "name" | "description" | "baseUrl" | "envKey" | "authType"
                ) {
                    entry.insert(key.clone(), value.clone());
                }
            }
            (target, Value::Object(entry))
        })
        .collect()
}

fn current_provider_in(document: &Value) -> Option<String> {
    let selected = document
        .pointer("/security/auth/selectedType")
        .and_then(Value::as_str)?;
    let name = document.pointer("/model/name").and_then(Value::as_str)?;
    let base = document.pointer("/model/baseUrl").and_then(Value::as_str);
    let entries = document.get("modelProviders")?.get(selected)?.as_array()?;
    let matches_id = |entry: &&Value| entry.get("id").and_then(Value::as_str) == Some(name);
    let entry = entries
        .iter()
        .filter(matches_id)
        .find(|entry| base.is_none() || entry.get("baseUrl").and_then(Value::as_str) == base)
        .or_else(|| entries.iter().find(matches_id))?;
    let env_key = entry.get("envKey").and_then(Value::as_str)?;
    groups(document)
        .into_iter()
        .find(|(_, group)| group.env_key == env_key)
        .map(|(id, _)| id)
}

pub(crate) fn current_provider() -> Result<String, AppError> {
    Ok(current_provider_in(&read(&config_path())?).unwrap_or_default())
}

/// Point `selectedType` / `model.name` / `model.baseUrl` at one of the provider's
/// models, keeping the current model when it already belongs to the provider.
fn select_provider_model(document: &mut Value, env_key: &str) -> Result<(), AppError> {
    let selected = document
        .pointer("/security/auth/selectedType")
        .and_then(Value::as_str)
        .map(str::to_string);
    let name = document
        .pointer("/model/name")
        .and_then(Value::as_str)
        .map(str::to_string);
    let base = document
        .pointer("/model/baseUrl")
        .and_then(Value::as_str)
        .map(str::to_string);
    let mut candidates: Vec<(String, Value)> = Vec::new();
    for (auth_type, entries) in document
        .get("modelProviders")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        for entry in entries.as_array().into_iter().flatten() {
            if entry.get("envKey").and_then(Value::as_str) == Some(env_key) {
                candidates.push((auth_type.clone(), entry.clone()));
            }
        }
    }
    let keep = candidates.iter().find(|(auth_type, entry)| {
        Some(auth_type) == selected.as_ref()
            && entry.get("id").and_then(Value::as_str) == name.as_deref()
            && (base.is_none() || entry.get("baseUrl").and_then(Value::as_str) == base.as_deref())
    });
    let (auth_type, entry) = keep
        .or_else(|| candidates.first())
        .cloned()
        .ok_or_else(|| AppError::InvalidInput("The Qwen Code provider has no models".into()))?;
    if !document.get("security").is_some_and(Value::is_object) {
        document["security"] = json!({});
    }
    if !document["security"]
        .get("auth")
        .is_some_and(Value::is_object)
    {
        document["security"]["auth"] = json!({});
    }
    document["security"]["auth"]["selectedType"] = json!(auth_type);
    if !document.get("model").is_some_and(Value::is_object) {
        document["model"] = json!({});
    }
    document["model"]["name"] = entry["id"].clone();
    match entry.get("baseUrl") {
        Some(url) => document["model"]["baseUrl"] = url.clone(),
        None => {
            document["model"].as_object_mut().unwrap().remove("baseUrl");
        }
    }
    Ok(())
}

fn apply_provider(
    before: &Value,
    id: &str,
    config: Option<&Value>,
    create: bool,
) -> Result<Value, AppError> {
    if let Some(config) = config {
        validate_provider(id, config)?;
    }
    let existing = groups(before);
    let env_key = match config {
        Some(config) => env_key_for(id, config),
        None => match existing.get(id) {
            Some(group) => group.env_key.clone(),
            None => return Ok(before.clone()),
        },
    };
    let live_owner = existing
        .iter()
        .find(|(_, group)| group.env_key == env_key)
        .map(|(key, _)| key.clone());
    if create && (live_owner.is_some() || existing.contains_key(id)) {
        return Err(AppError::InvalidInput(format!(
            "Qwen Code provider key '{id}' already exists"
        )));
    }
    let was_current =
        live_owner.is_some() && current_provider_in(before).as_deref() == live_owner.as_deref();
    if config.is_none() && was_current {
        return Err(AppError::InvalidInput(
            "Switch Qwen Code to another provider before removing the one in use".into(),
        ));
    }

    let entries = config
        .map(|config| native_entries(config, &env_key))
        .unwrap_or_default();
    let mut document = before.clone();
    if !document.get("modelProviders").is_some_and(Value::is_object) {
        document["modelProviders"] = json!({});
    }
    let providers = document["modelProviders"].as_object_mut().unwrap();
    // The same model at the same endpoint under another key would be ambiguous.
    for (auth_type, entry) in &entries {
        let clash = providers
            .get(auth_type)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .any(|other| {
                other.get("envKey").and_then(Value::as_str) != Some(env_key.as_str())
                    && other.get("id") == entry.get("id")
                    && other.get("baseUrl") == entry.get("baseUrl")
            });
        if clash {
            return Err(AppError::InvalidInput(format!(
                "Qwen Code model '{}' at this base URL already belongs to another provider",
                entry["id"].as_str().unwrap_or_default()
            )));
        }
    }
    let mut emptied = Vec::new();
    for (auth_type, list) in providers.iter_mut() {
        if let Some(list) = list.as_array_mut() {
            let len = list.len();
            list.retain(|entry| {
                entry.get("envKey").and_then(Value::as_str) != Some(env_key.as_str())
            });
            if len != list.len() && list.is_empty() {
                emptied.push(auth_type.clone());
            }
        }
    }
    for (auth_type, entry) in &entries {
        let list = providers
            .entry(auth_type.clone())
            .or_insert_with(|| json!([]));
        if !list.is_array() {
            *list = json!([]);
        }
        list.as_array_mut().unwrap().push(entry.clone());
    }
    for auth_type in emptied {
        if providers
            .get(&auth_type)
            .and_then(Value::as_array)
            .is_some_and(Vec::is_empty)
        {
            providers.remove(&auth_type);
        }
    }
    if providers.is_empty() && before.get("modelProviders").is_none() {
        document.as_object_mut().unwrap().remove("modelProviders");
    }

    let managed = env_key.starts_with(MANAGED_ENV_PREFIX);
    let api_key = config
        .and_then(|config| config.get("apiKey"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|key| !key.is_empty());
    if let Some(api_key) = api_key {
        if !document.get("env").is_some_and(Value::is_object) {
            document["env"] = json!({});
        }
        document["env"][&env_key] = json!(api_key);
    } else if managed {
        let env = document.get_mut("env").and_then(Value::as_object_mut);
        if let Some(env) = env {
            if env.remove(&env_key).is_some() && env.is_empty() {
                document.as_object_mut().unwrap().remove("env");
            }
        }
    }

    if config.is_some() {
        let nothing_selected = document
            .pointer("/security/auth/selectedType")
            .and_then(Value::as_str)
            .is_none()
            && document
                .pointer("/model/name")
                .and_then(Value::as_str)
                .is_none();
        if was_current || nothing_selected {
            select_provider_model(&mut document, &env_key)?;
        }
    }
    Ok(document)
}

pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock()?;
    let path = config_path();
    update_and_commit(
        &path,
        |before| apply_provider(before, id, config.as_ref(), create),
        |_| commit(),
    )
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}

/// "Add" writes a saved provider into settings.json without selecting it, so its
/// models show up in Qwen Code's /model list. Entries already there belong to
/// Qwen and are kept as is (same rule as switching).
pub(crate) fn add_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock()?;
    add_at(&config_path(), id, archived_config, commit)
}

fn add_at<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    update_and_commit(
        path,
        |before| {
            if groups(before).contains_key(id) {
                Ok(before.clone())
            } else {
                apply_provider(before, id, Some(&archived_config), false)
            }
        },
        |_| commit(),
    )
}

pub(crate) fn switch_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock()?;
    switch_at(&config_path(), id, archived_config, commit)
}

fn switch_at<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let env_key = std::cell::RefCell::new(String::new());
    update_and_commit(
        path,
        |before| {
            // Already-live entries belong to Qwen: a card click must not overwrite
            // changes made in Qwen since the last provider-list refresh.
            let mut after = match groups(before).get(id) {
                Some(group) => {
                    *env_key.borrow_mut() = group.env_key.clone();
                    before.clone()
                }
                None => {
                    *env_key.borrow_mut() = env_key_for(id, &archived_config);
                    apply_provider(before, id, Some(&archived_config), false)?
                }
            };
            select_provider_model(&mut after, &env_key.borrow())?;
            Ok(after)
        },
        |after| {
            let config = groups(after)
                .into_values()
                .find(|group| group.env_key == *env_key.borrow())
                .map(|group| group.config)
                .unwrap_or(Value::Null);
            commit(config)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(base: &str, key: &str, models: &[&str]) -> Value {
        json!({
            "authType": "openai",
            "baseUrl": base,
            "apiKey": key,
            "models": models.iter().map(|id| json!({"id": id, "generationConfig": {"contextWindowSize": 128000}})).collect::<Vec<_>>(),
        })
    }

    #[test]
    fn env_key_round_trips_provider_keys() {
        assert_eq!(managed_env_key("dash-scope2"), "CC_SWITCH_QWEN_DASH_SCOPE2");
        assert_eq!(
            provider_id_for_env_key("CC_SWITCH_QWEN_DASH_SCOPE2"),
            "dash-scope2"
        );
        assert_eq!(provider_id_for_env_key("OPENAI_API_KEY"), "openai-api-key");
    }

    #[test]
    fn add_writes_entries_env_and_selects_first_provider() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(
            &path,
            "// user comment\n{\n  \"general\": { \"vimMode\": true },\n  \"modelProviders\": {\n    \"openai\": [{ \"id\": \"gpt-x\", \"envKey\": \"OPENAI_API_KEY\", \"baseUrl\": \"https://api.openai.com/v1\" }]\n  },\n}\n",
        )
        .unwrap();
        update_and_commit(
            &path,
            |before| {
                apply_provider(
                    before,
                    "bailian",
                    Some(&provider(
                        "https://dashscope.aliyuncs.com/compatible-mode/v1",
                        "sk-1",
                        &["qwen3-coder-plus"],
                    )),
                    true,
                )
            },
            |_| Ok(()),
        )
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("// user comment"));
        let value = read(&path).unwrap();
        assert_eq!(value["general"]["vimMode"], true);
        assert_eq!(value["env"]["CC_SWITCH_QWEN_BAILIAN"], "sk-1");
        let entries = value["modelProviders"]["openai"].as_array().unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[1]["envKey"], "CC_SWITCH_QWEN_BAILIAN");
        assert_eq!(entries[1]["generationConfig"]["contextWindowSize"], 128000);
        assert_eq!(value["security"]["auth"]["selectedType"], "openai");
        assert_eq!(value["model"]["name"], "qwen3-coder-plus");
        assert_eq!(current_provider_in(&value).as_deref(), Some("bailian"));

        let groups = groups(&value);
        assert_eq!(
            groups.keys().collect::<Vec<_>>(),
            ["openai-api-key", "bailian"]
        );
        assert_eq!(groups["openai-api-key"].config["envKey"], "OPENAI_API_KEY");
        assert!(groups["bailian"].config.get("envKey").is_none());
        assert_eq!(groups["bailian"].config["apiKey"], "sk-1");
        assert!(groups["bailian"].config["models"][0]
            .get("baseUrl")
            .is_none());
    }

    #[test]
    fn edit_preserves_unknown_fields_and_other_auth_types() {
        let mut config = provider("https://a.example/v1", "k", &["m1"]);
        config["models"][0]["capabilities"] = json!({"vision": true});
        config["models"].as_array_mut().unwrap().push(
            json!({"id": "c1", "authType": "anthropic", "baseUrl": "https://a.example/anthropic"}),
        );
        let after = apply_provider(&json!({}), "p", Some(&config), true).unwrap();
        assert_eq!(after["modelProviders"]["anthropic"][0]["id"], "c1");
        assert_eq!(
            after["modelProviders"]["openai"][0]["capabilities"]["vision"],
            true
        );
        let round = groups(&after).swap_remove("p").unwrap().config;
        assert_eq!(round["models"][1]["authType"], "anthropic");
        assert_eq!(round["models"][1]["baseUrl"], "https://a.example/anthropic");
        let again = apply_provider(&after, "p", Some(&round), false).unwrap();
        assert_eq!(again["modelProviders"], after["modelProviders"]);
    }

    #[test]
    fn remove_refuses_current_and_drops_only_managed_env() {
        let first = apply_provider(
            &json!({}),
            "a",
            Some(&provider("https://a/v1", "ka", &["m"])),
            true,
        )
        .unwrap();
        let both = apply_provider(
            &first,
            "b",
            Some(&provider("https://b/v1", "kb", &["m"])),
            true,
        )
        .unwrap();
        assert_eq!(current_provider_in(&both).as_deref(), Some("a"));
        assert!(apply_provider(&both, "a", None, false).is_err());
        let removed = apply_provider(&both, "b", None, false).unwrap();
        assert!(removed["env"].get("CC_SWITCH_QWEN_B").is_none());
        assert_eq!(removed["env"]["CC_SWITCH_QWEN_A"], "ka");
        assert_eq!(
            removed["modelProviders"]["openai"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn switch_selects_model_with_base_url_for_duplicate_ids() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let first = apply_provider(
            &json!({}),
            "a",
            Some(&provider("https://a/v1", "ka", &["m"])),
            true,
        )
        .unwrap();
        let both = apply_provider(
            &first,
            "b",
            Some(&provider("https://b/v1", "kb", &["m"])),
            true,
        )
        .unwrap();
        fs::write(&path, serde_json::to_string(&both).unwrap()).unwrap();
        let config = switch_at(&path, "b", Value::Null, Ok).unwrap();
        assert_eq!(config["baseUrl"], "https://b/v1");
        let value = read(&path).unwrap();
        assert_eq!(value["model"]["baseUrl"], "https://b/v1");
        assert_eq!(current_provider_in(&value).as_deref(), Some("b"));
    }

    #[test]
    fn add_keeps_the_selected_model_and_never_overwrites_live_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let first = apply_provider(
            &json!({}),
            "a",
            Some(&provider("https://a/v1", "ka", &["m"])),
            true,
        )
        .unwrap();
        let native = format!(
            "// original\n{}",
            serde_json::to_string_pretty(&first).unwrap()
        );
        fs::write(&path, &native).unwrap();
        let failed = add_at(&path, "b", provider("https://b/v1", "kb", &["n"]), || {
            Err::<(), _>(AppError::Message("database failed".into()))
        });
        assert!(failed.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), native);

        add_at(
            &path,
            "b",
            provider("https://b/v1", "kb", &["n"]),
            || Ok(()),
        )
        .unwrap();
        let value = read(&path).unwrap();
        assert_eq!(current_provider_in(&value).as_deref(), Some("a"));
        assert_eq!(value["model"]["name"], "m");
        assert!(groups(&value).contains_key("b"));
        assert!(fs::read_to_string(&path).unwrap().contains("// original"));

        // Entries already in Qwen belong to Qwen: adding again keeps them.
        let before = fs::read_to_string(&path).unwrap();
        add_at(
            &path,
            "b",
            provider("https://stale/v1", "old", &["n"]),
            || Ok(()),
        )
        .unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn duplicate_key_and_validation_errors() {
        let first = apply_provider(
            &json!({}),
            "a",
            Some(&provider("https://a/v1", "", &["m"])),
            true,
        )
        .unwrap();
        assert!(apply_provider(
            &first,
            "a",
            Some(&provider("https://x/v1", "", &["n"])),
            true
        )
        .is_err());
        assert!(apply_provider(
            &first,
            "c",
            Some(&provider("https://a/v1", "", &["m"])),
            true
        )
        .is_err());
        assert!(validate_provider("Bad_Key", &provider("https://a/v1", "", &["m"])).is_err());
        let mut anthropic = provider("", "", &["m"]);
        anthropic["authType"] = json!("anthropic");
        assert!(validate_provider("x", &anthropic).is_err());
        assert!(validate_provider("x", &json!({"authType": "openai", "models": []})).is_err());
    }

    #[test]
    fn failed_commit_restores_file_and_malformed_file_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        fs::write(&path, "{ \"a\": 1 }").unwrap();
        let result: Result<(), AppError> = update_and_commit(
            &path,
            |before| {
                apply_provider(
                    before,
                    "x",
                    Some(&provider("https://x/v1", "k", &["m"])),
                    true,
                )
            },
            |_| Err(AppError::Message("db".into())),
        );
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "{ \"a\": 1 }");
        fs::write(&path, "{invalid").unwrap();
        assert!(read(&path).is_err());
    }

    #[test]
    fn new_file_is_stamped_with_settings_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        update_and_commit(
            &path,
            |before| {
                apply_provider(
                    before,
                    "x",
                    Some(&provider("https://x/v1", "k", &["m"])),
                    true,
                )
            },
            |_| Ok(()),
        )
        .unwrap();
        assert_eq!(read(&path).unwrap()["$version"], SETTINGS_VERSION);
    }
}
