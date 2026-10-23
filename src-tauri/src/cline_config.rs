//! Cline CLI keeps provider credentials and the current selection in
//! `providers.json`, and declares custom providers in `models.json` next to it.
//!
//! A saved CCS provider is `{ settings, tokenSource?, provider?, models? }`:
//! `settings` is the native `providers.json` entry settings (its `provider`
//! field is always the CCS id), and `provider` + `models` are present only for
//! custom providers declared in `models.json`. Built-in Cline providers
//! (`anthropic`, `openrouter`, …) carry no `provider` block and use the
//! built-in id as the CCS id.
//!
//! Cline rewrites `providers.json` on every run, drops unknown keys, and resets
//! the whole file to its default when any field has the wrong type, so every
//! write here is validated against that schema and the previous file is kept as
//! a backup next to it.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Prefix for custom providers created by CC Switch, so they never collide with
/// Cline's built-in provider ids.
pub(crate) const CUSTOM_ID_PREFIX: &str = "ccs-";
const BACKUP_SUFFIX: &str = "cc-switch.bak";
const PROTOCOLS: &[&str] = &[
    "openai-responses",
    "openai-chat",
    "anthropic",
    "gemini",
    "openai-r1",
    "ai-sdk",
];
const CLIENTS: &[&str] = &[
    "custom",
    "anthropic",
    "gemini",
    "openai-r1",
    "ai-sdk",
    "ai-sdk-community",
    "openai",
    "openai-compatible",
    "bedrock",
    "fetch",
    "vertex",
];
const TOKEN_SOURCES: &[&str] = &["manual", "oauth", "migration"];
const MODEL_NUMBER_FIELDS: &[&str] = &[
    "maxTokens",
    "contextWindow",
    "maxInputTokens",
    "inputPrice",
    "outputPrice",
    "cacheReadsPrice",
    "cacheWritesPrice",
    "temperature",
];

fn env_path(key: &str) -> Option<PathBuf> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// `$CLINE_DIR`, default `~/.cline`. Rules, skills and hooks live here.
pub(crate) fn cline_dir() -> PathBuf {
    env_path("CLINE_DIR").unwrap_or_else(|| get_home_dir().join(".cline"))
}

/// `$CLINE_DATA_DIR`, default `<cline dir>/data`.
pub(crate) fn data_dir() -> PathBuf {
    env_path("CLINE_DATA_DIR").unwrap_or_else(|| cline_dir().join("data"))
}

pub(crate) fn providers_path() -> PathBuf {
    env_path("CLINE_PROVIDER_SETTINGS_PATH")
        .unwrap_or_else(|| data_dir().join("settings").join("providers.json"))
}

/// Cline resolves `models.json` next to `providers.json`.
pub(crate) fn models_path() -> PathBuf {
    providers_path()
        .parent()
        .map(|dir| dir.join("models.json"))
        .unwrap_or_else(|| data_dir().join("settings").join("models.json"))
}

pub(crate) fn mcp_path() -> PathBuf {
    env_path("CLINE_MCP_SETTINGS_PATH")
        .unwrap_or_else(|| data_dir().join("settings").join("cline_mcp_settings.json"))
}

pub(crate) fn sessions_dir() -> PathBuf {
    env_path("CLINE_SESSION_DATA_DIR").unwrap_or_else(|| data_dir().join("sessions"))
}

pub(crate) fn sessions_db_path() -> PathBuf {
    env_path("CLINE_DB_DATA_DIR")
        .unwrap_or_else(|| data_dir().join("db"))
        .join("sessions.db")
}

pub(crate) fn skills_dir() -> PathBuf {
    cline_dir().join("skills")
}

/// Every Markdown file here is injected into Cline's system prompt.
pub(crate) fn rules_dir() -> PathBuf {
    cline_dir().join("rules")
}

fn read_json(path: &Path, what: &str) -> Result<Option<Value>, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(AppError::io(path, error)),
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    let value: Value = serde_json::from_str(&text)
        .map_err(|_| AppError::Config(format!("Invalid Cline {what} JSON")))?;
    if !value.is_object() || value.get("providers").is_some_and(|p| !p.is_object()) {
        return Err(AppError::Config(format!(
            "Cline {what} must contain a providers object"
        )));
    }
    Ok(Some(value))
}

#[derive(Debug, Clone, PartialEq)]
struct Documents {
    providers: Value,
    models: Value,
}

fn read_documents(providers_path: &Path, models_path: &Path) -> Result<Documents, AppError> {
    let providers = read_json(providers_path, "providers.json")?
        .unwrap_or_else(|| json!({"version": 1, "modes": {}, "providers": {}}));
    let models = read_json(models_path, "models.json")?
        .unwrap_or_else(|| json!({"version": 1, "providers": {}}));
    Ok(Documents { providers, models })
}

fn entry_config(documents: &Documents, id: &str) -> Option<Value> {
    let entry = documents
        .providers
        .pointer(&format!("/providers/{}", escape(id)));
    let custom = documents
        .models
        .pointer(&format!("/providers/{}", escape(id)));
    if entry.is_none() && custom.is_none() {
        return None;
    }
    let mut config = Map::new();
    let mut settings = entry
        .and_then(|entry| entry.get("settings"))
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_default();
    settings.remove("provider");
    config.insert("settings".into(), Value::Object(settings));
    if let Some(source) = entry.and_then(|entry| entry.get("tokenSource")) {
        config.insert("tokenSource".into(), source.clone());
    }
    if let Some(custom) = custom {
        config.insert(
            "provider".into(),
            custom.get("provider").cloned().unwrap_or_else(|| json!({})),
        );
        config.insert(
            "models".into(),
            custom.get("models").cloned().unwrap_or_else(|| json!({})),
        );
    }
    Some(Value::Object(config))
}

/// JSON pointer escaping for provider ids used as object keys.
fn escape(id: &str) -> String {
    id.replace('~', "~0").replace('/', "~1")
}

fn ids(documents: &Documents) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for doc in [&documents.providers, &documents.models] {
        for id in doc
            .get("providers")
            .and_then(Value::as_object)
            .into_iter()
            .flatten()
            .map(|(id, _)| id)
        {
            if !ids.contains(id) {
                ids.push(id.clone());
            }
        }
    }
    ids
}

/// All providers known to Cline: built-in entries in `providers.json` and
/// custom providers in `models.json`.
pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let documents = read_documents(&providers_path(), &models_path())?;
    Ok(ids(&documents)
        .into_iter()
        .filter_map(|id| entry_config(&documents, &id).map(|config| (id, config)))
        .collect())
}

pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    let documents = read_documents(&providers_path(), &models_path())?;
    Ok(entry_config(&documents, id).is_some())
}

pub(crate) fn is_custom(config: &Value) -> bool {
    config.get("provider").is_some_and(Value::is_object)
}

/// Display name for list rows: the custom provider name, otherwise the id.
pub(crate) fn display_name(id: &str, config: &Value) -> String {
    config
        .pointer("/provider/name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .unwrap_or(id)
        .to_string()
}

fn valid_url(value: &Value) -> bool {
    value.as_str().is_some_and(|base| {
        url::Url::parse(base)
            .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    })
}

fn string_map(value: &Value) -> bool {
    value
        .as_object()
        .is_some_and(|map| map.values().all(Value::is_string))
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() || id != id.trim() || id.chars().any(char::is_control) {
        return Err(AppError::InvalidInput("Enter a Cline provider ID".into()));
    }
    let settings = config
        .get("settings")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("Cline provider settings are missing".into()))?;
    for key in ["apiKey", "model", "baseUrl", "region", "protocol", "client"] {
        if settings.get(key).is_some_and(|value| !value.is_string()) {
            return Err(AppError::InvalidInput(format!(
                "Cline provider setting '{key}' must be text"
            )));
        }
    }
    if settings
        .get("baseUrl")
        .is_some_and(|base| !base.as_str().unwrap_or_default().is_empty() && !valid_url(base))
    {
        return Err(AppError::InvalidInput(
            "Enter a valid Cline endpoint URL".into(),
        ));
    }
    if settings.get("headers").is_some_and(|h| !string_map(h)) {
        return Err(AppError::InvalidInput(
            "Cline request headers must be text values".into(),
        ));
    }
    for key in ["maxTokens", "contextWindow", "timeout"] {
        if settings.get(key).is_some_and(|value| !value.is_number()) {
            return Err(AppError::InvalidInput(format!(
                "Cline provider setting '{key}' must be a number"
            )));
        }
    }
    if config.get("tokenSource").is_some_and(|source| {
        !source
            .as_str()
            .is_some_and(|source| TOKEN_SOURCES.contains(&source))
    }) {
        return Err(AppError::InvalidInput(
            "Unsupported Cline token source".into(),
        ));
    }
    if !is_custom(config) {
        if config.get("models").is_some() {
            return Err(AppError::InvalidInput(
                "Cline built-in providers cannot declare models".into(),
            ));
        }
        return Ok(());
    }
    let provider = &config["provider"];
    if provider
        .get("name")
        .and_then(Value::as_str)
        .is_none_or(|name| name.trim().is_empty())
    {
        return Err(AppError::InvalidInput("Enter a Cline provider name".into()));
    }
    if !provider.get("baseUrl").is_some_and(valid_url) {
        return Err(AppError::InvalidInput(
            "Enter a valid Cline endpoint URL".into(),
        ));
    }
    if provider.get("protocol").is_some_and(|protocol| {
        !protocol
            .as_str()
            .is_some_and(|protocol| PROTOCOLS.contains(&protocol))
    }) {
        return Err(AppError::InvalidInput(
            "Select a supported Cline protocol".into(),
        ));
    }
    if provider.get("client").is_some_and(|client| {
        !client
            .as_str()
            .is_some_and(|client| CLIENTS.contains(&client))
    }) {
        return Err(AppError::InvalidInput("Unsupported Cline client".into()));
    }
    // Without `client`, Cline sends Anthropic-protocol providers to /chat/completions.
    if provider.get("protocol").and_then(Value::as_str) == Some("anthropic")
        && provider.get("client").and_then(Value::as_str) != Some("anthropic")
    {
        return Err(AppError::InvalidInput(
            "Anthropic-protocol Cline providers need client \"anthropic\"".into(),
        ));
    }
    if provider
        .get("defaultModelId")
        .is_some_and(|id| !id.is_string())
    {
        return Err(AppError::InvalidInput(
            "Cline default model must be text".into(),
        ));
    }
    let models = config
        .get("models")
        .and_then(Value::as_object)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Add at least one Cline model".into()))?;
    for (key, model) in models {
        if key.trim().is_empty() || !model.is_object() {
            return Err(AppError::InvalidInput(
                "Cline models need a model ID".into(),
            ));
        }
        if model
            .get("id")
            .is_some_and(|value| value.as_str() != Some(key.as_str()))
            || model.get("name").is_some_and(|name| !name.is_string())
        {
            return Err(AppError::InvalidInput(format!(
                "Cline model '{key}' has an invalid id or name"
            )));
        }
        for field in MODEL_NUMBER_FIELDS {
            if model.get(*field).is_some_and(|value| !value.is_number()) {
                return Err(AppError::InvalidInput(format!(
                    "Cline model '{key}' field '{field}' must be a number"
                )));
            }
        }
        if model.get("capabilities").is_some_and(|caps| {
            caps.as_array()
                .is_none_or(|caps| caps.iter().any(|cap| !cap.is_string()))
        }) {
            return Err(AppError::InvalidInput(format!(
                "Cline model '{key}' capabilities must be a list"
            )));
        }
    }
    if let Some(model) = settings.get("model").and_then(Value::as_str) {
        if !model.is_empty() && !models.contains_key(model) {
            return Err(AppError::InvalidInput(format!(
                "Cline model '{model}' is not declared for this provider"
            )));
        }
    }
    Ok(())
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

fn ensure_object<'a>(doc: &'a mut Value, key: &str) -> &'a mut Map<String, Value> {
    if !doc.get(key).is_some_and(Value::is_object) {
        doc[key] = json!({});
    }
    doc[key].as_object_mut().unwrap()
}

fn apply_provider(
    before: &Documents,
    id: &str,
    config: Option<Value>,
    create: bool,
) -> Result<Documents, AppError> {
    if let Some(config) = &config {
        validate_provider(id, config)?;
    }
    let exists = entry_config(before, id).is_some();
    if config.is_none() && !exists {
        return Ok(before.clone());
    }
    if create && exists {
        return Err(AppError::InvalidInput(format!(
            "Cline provider ID '{id}' already exists"
        )));
    }
    let current = before
        .providers
        .get("lastUsedProvider")
        .and_then(Value::as_str);
    if config.is_none() && current == Some(id) {
        return Err(AppError::InvalidInput(
            "Switch Cline to another provider before removing this one".into(),
        ));
    }
    let mut after = before.clone();
    for doc in [&mut after.providers, &mut after.models] {
        if doc.get("version").is_none() {
            doc["version"] = json!(1);
        }
    }
    if after.providers.get("modes").is_none() {
        after.providers["modes"] = json!({});
    }
    let Some(config) = config else {
        ensure_object(&mut after.providers, "providers").remove(id);
        ensure_object(&mut after.models, "providers").remove(id);
        return Ok(after);
    };

    let mut settings = config["settings"].as_object().cloned().unwrap_or_default();
    settings.insert("provider".into(), json!(id));
    let previous_entry = before
        .providers
        .pointer(&format!("/providers/{}", escape(id)))
        .cloned();
    let mut entry = previous_entry
        .clone()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let token_source = config
        .get("tokenSource")
        .cloned()
        .or_else(|| entry.get("tokenSource").cloned())
        .unwrap_or_else(|| json!("manual"));
    let changed = previous_entry
        .as_ref()
        .and_then(|entry| entry.get("settings"))
        != Some(&Value::Object(settings.clone()))
        || entry.get("tokenSource") != Some(&token_source);
    entry["settings"] = Value::Object(settings);
    entry["tokenSource"] = token_source;
    if changed || entry.get("updatedAt").is_none() {
        entry["updatedAt"] = json!(now());
    }
    ensure_object(&mut after.providers, "providers").insert(id.into(), entry);

    let custom_map = ensure_object(&mut after.models, "providers");
    if is_custom(&config) {
        let mut custom = custom_map
            .get(id)
            .cloned()
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        custom["provider"] = config["provider"].clone();
        let mut models = config["models"].as_object().cloned().unwrap_or_default();
        for (key, model) in models.iter_mut() {
            if model.get("id").is_none() {
                model["id"] = json!(key);
            }
        }
        custom["models"] = Value::Object(models);
        custom_map.insert(id.into(), custom);
    } else {
        custom_map.remove(id);
    }
    if after
        .providers
        .get("lastUsedProvider")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        after.providers["lastUsedProvider"] = json!(id);
    }
    Ok(after)
}

fn pretty(value: &Value) -> Result<Vec<u8>, AppError> {
    let mut text =
        serde_json::to_string_pretty(value).map_err(|e| AppError::Config(e.to_string()))?;
    text.push('\n');
    Ok(text.into_bytes())
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "providers.json".into());
    path.with_file_name(format!("{name}.{BACKUP_SUFFIX}"))
}

fn snapshot(path: &Path) -> Result<Option<Vec<u8>>, AppError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AppError::io(path, error)),
    }
}

fn restore(path: &Path, previous: &Option<Vec<u8>>) -> Result<(), AppError> {
    match previous {
        Some(bytes) => atomic_write_private(path, bytes),
        None => match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::io(path, error)),
        },
    }
}

/// Write both documents, then run `commit` (the database save). Restores both
/// files when any step fails.
fn write_and_commit<T>(
    paths: (&Path, &Path),
    before: &Documents,
    after: &Documents,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let (providers_path, models_path) = paths;
    let previous_providers = snapshot(providers_path)?;
    let previous_models = snapshot(models_path)?;
    let result = (|| {
        if before.providers != after.providers {
            if let Some(bytes) = &previous_providers {
                atomic_write_private(&backup_path(providers_path), bytes)?;
            }
            atomic_write_private(providers_path, &pretty(&after.providers)?)?;
        }
        if before.models != after.models {
            atomic_write_private(models_path, &pretty(&after.models)?)?;
        }
        commit()
    })();
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let rollback = restore(providers_path, &previous_providers)
                .and_then(|_| restore(models_path, &previous_models));
            if let Err(rollback_error) = rollback {
                return Err(AppError::Message(format!(
                    "Cline config update failed ({error}); restoring the previous files also failed: {rollback_error}"
                )));
            }
            Err(error)
        }
    }
}

fn lock() -> Result<std::sync::MutexGuard<'static, ()>, AppError> {
    WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))
}

pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock()?;
    let (providers_path, models_path) = (providers_path(), models_path());
    save_at((&providers_path, &models_path), id, config, create, commit)
}

fn save_at<T>(
    paths: (&Path, &Path),
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let before = read_documents(paths.0, paths.1)?;
    let after = apply_provider(&before, id, config, create)?;
    write_and_commit(paths, &before, &after, commit)
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}

pub(crate) fn current_provider() -> Result<String, AppError> {
    let documents = read_documents(&providers_path(), &models_path())?;
    Ok(current_provider_in(&documents).unwrap_or_default().into())
}

fn current_provider_in(documents: &Documents) -> Option<&str> {
    let id = documents
        .providers
        .get("lastUsedProvider")?
        .as_str()
        .filter(|id| !id.is_empty())?;
    documents
        .providers
        .pointer(&format!("/providers/{}", escape(id)))?;
    Some(id)
}

/// Select `id` as Cline's current provider. A provider that is already live
/// belongs to Cline: the archived copy only fills it in when it is missing.
pub(crate) fn switch_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock()?;
    let (providers_path, models_path) = (providers_path(), models_path());
    switch_at((&providers_path, &models_path), id, archived_config, commit)
}

fn switch_at<T>(
    paths: (&Path, &Path),
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let before = read_documents(paths.0, paths.1)?;
    let live = before
        .providers
        .pointer(&format!("/providers/{}", escape(id)))
        .is_some();
    let mut after = if live {
        before.clone()
    } else {
        apply_provider(&before, id, Some(archived_config), false)?
    };
    after.providers["lastUsedProvider"] = json!(id);
    let config = entry_config(&after, id)
        .ok_or_else(|| AppError::InvalidInput(format!("Cline provider '{id}' not found")))?;
    write_and_commit(paths, &before, &after, || commit(config))
}

/// "Add" writes a saved provider into Cline without selecting it. An entry that
/// is already live belongs to Cline and is kept as is (same rule as switching).
pub(crate) fn add_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock()?;
    let (providers_path, models_path) = (providers_path(), models_path());
    add_at((&providers_path, &models_path), id, archived_config, commit)
}

fn add_at<T>(
    paths: (&Path, &Path),
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let before = read_documents(paths.0, paths.1)?;
    let after = if entry_config(&before, id).is_some() {
        before.clone()
    } else {
        apply_provider(&before, id, Some(archived_config), false)?
    };
    write_and_commit(paths, &before, &after, commit)
}

/// Base URL and API key for connectivity checks and model listing.
pub(crate) fn endpoint_and_key(config: &Value) -> (String, String) {
    let base = config
        .pointer("/provider/baseUrl")
        .or_else(|| config.pointer("/settings/baseUrl"))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let key = config
        .pointer("/settings/apiKey")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    (base, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn paths(dir: &TempDir) -> (PathBuf, PathBuf) {
        (
            dir.path().join("providers.json"),
            dir.path().join("models.json"),
        )
    }

    fn custom(model: &str) -> Value {
        json!({
            "settings": {"apiKey": "sk-test", "model": model, "headers": {"X-Test": "1"}},
            "provider": {"name": "Relay", "baseUrl": "https://relay.example.com/v1",
                         "defaultModelId": model, "protocol": "anthropic", "client": "anthropic"},
            "models": {model: {"name": "M", "contextWindow": 200000, "inputPrice": 3}}
        })
    }

    fn save(dir: &TempDir, id: &str, config: Option<Value>, create: bool) -> Result<(), AppError> {
        let (p, m) = paths(dir);
        save_at((&p, &m), id, config, create, || Ok(()))
    }

    fn docs(dir: &TempDir) -> Documents {
        let (p, m) = paths(dir);
        read_documents(&p, &m).unwrap()
    }

    #[test]
    fn custom_provider_writes_both_files_with_native_shape() {
        let dir = TempDir::new().unwrap();
        save(&dir, "ccs-relay", Some(custom("m1")), true).unwrap();
        let d = docs(&dir);
        assert_eq!(d.providers["version"], 1);
        assert_eq!(d.providers["lastUsedProvider"], "ccs-relay");
        let entry = &d.providers["providers"]["ccs-relay"];
        assert_eq!(entry["settings"]["provider"], "ccs-relay");
        assert_eq!(entry["settings"]["apiKey"], "sk-test");
        assert_eq!(entry["tokenSource"], "manual");
        assert!(entry["updatedAt"].is_string());
        assert_eq!(
            d.models["providers"]["ccs-relay"]["provider"]["client"],
            "anthropic"
        );
        assert_eq!(
            d.models["providers"]["ccs-relay"]["models"]["m1"]["id"],
            "m1"
        );
        // Round-trip back to the CCS shape without the forced `provider` field.
        let config = entry_config(&d, "ccs-relay").unwrap();
        assert!(config["settings"].get("provider").is_none());
        assert_eq!(config["models"]["m1"]["contextWindow"], 200000);
    }

    #[test]
    fn edits_preserve_other_providers_and_unknown_native_fields() {
        let dir = TempDir::new().unwrap();
        let (p, m) = paths(&dir);
        fs::write(
            &p,
            json!({"version": 1, "lastUsedProvider": "cline", "modes": {"voiceInput": {"providerId": "x", "modelId": "y"}},
                   "providers": {"cline": {"settings": {"provider": "cline", "auth": {"accessToken": "t"}},
                                           "updatedAt": "2026-01-01T00:00:00.000Z", "tokenSource": "oauth"}}})
            .to_string(),
        )
        .unwrap();
        fs::write(
            &m,
            json!({"version": 1, "providers": {"user-own": {"provider": {"name": "Own", "baseUrl": "https://own.example"},
                   "models": {}, "discoveredModelIds": ["a"]}}})
            .to_string(),
        )
        .unwrap();
        save(&dir, "ccs-relay", Some(custom("m1")), true).unwrap();
        let d = docs(&dir);
        assert_eq!(d.providers["lastUsedProvider"], "cline");
        assert_eq!(d.providers["modes"]["voiceInput"]["modelId"], "y");
        assert_eq!(d.providers["providers"]["cline"]["tokenSource"], "oauth");
        assert_eq!(
            d.providers["providers"]["cline"]["updatedAt"],
            "2026-01-01T00:00:00.000Z"
        );
        assert_eq!(
            d.models["providers"]["user-own"]["discoveredModelIds"][0],
            "a"
        );
        // The previous providers.json is kept next to it.
        assert!(backup_path(&p).exists());
    }

    #[test]
    fn rejects_values_that_would_make_cline_reset_its_file() {
        let mut bad = custom("m1");
        bad["settings"]["model"] = json!(123);
        assert!(validate_provider("ccs-x", &bad).is_err());
        let mut bad = custom("m1");
        bad["settings"]["headers"] = json!({"X": 1});
        assert!(validate_provider("ccs-x", &bad).is_err());
        let mut bad = custom("m1");
        bad["models"]["m1"]["contextWindow"] = json!("big");
        assert!(validate_provider("ccs-x", &bad).is_err());
        let mut bad = custom("m1");
        bad["provider"].as_object_mut().unwrap().remove("client");
        assert!(
            validate_provider("ccs-x", &bad).is_err(),
            "anthropic needs client"
        );
        let mut bad = custom("m1");
        bad["settings"]["model"] = json!("undeclared");
        assert!(validate_provider("ccs-x", &bad).is_err());
        assert!(validate_provider(" ccs-x", &custom("m1")).is_err());
        assert!(validate_provider("ccs-x", &custom("m1")).is_ok());
    }

    #[test]
    fn built_in_provider_only_touches_providers_json() {
        let dir = TempDir::new().unwrap();
        let config = json!({"settings": {"apiKey": "sk-ant", "model": "claude-sonnet-4-5",
                                          "baseUrl": "https://api.anthropic.com/v1"}});
        save(&dir, "anthropic", Some(config), true).unwrap();
        let d = docs(&dir);
        assert_eq!(
            d.providers["providers"]["anthropic"]["settings"]["provider"],
            "anthropic"
        );
        assert!(d.models["providers"].get("anthropic").is_none());
        let (_, m) = paths(&dir);
        assert!(
            !m.exists(),
            "models.json is not created for built-in providers"
        );
    }

    #[test]
    fn duplicate_create_and_removing_current_are_rejected() {
        let dir = TempDir::new().unwrap();
        save(&dir, "ccs-a", Some(custom("m1")), true).unwrap();
        assert!(save(&dir, "ccs-a", Some(custom("m1")), true).is_err());
        assert!(
            save(&dir, "ccs-a", None, false).is_err(),
            "current provider"
        );
        save(&dir, "ccs-b", Some(custom("m2")), true).unwrap();
        let (p, m) = paths(&dir);
        switch_at((&p, &m), "ccs-b", json!({}), |_| Ok(())).unwrap();
        save(&dir, "ccs-a", None, false).unwrap();
        let d = docs(&dir);
        assert!(d.providers["providers"].get("ccs-a").is_none());
        assert!(d.models["providers"].get("ccs-a").is_none());
        assert_eq!(current_provider_in(&d), Some("ccs-b"));
    }

    #[test]
    fn switch_keeps_live_config_and_restores_archived_one() {
        let dir = TempDir::new().unwrap();
        let (p, m) = paths(&dir);
        save(&dir, "ccs-a", Some(custom("m1")), true).unwrap();
        // Archived copy is ignored for a live provider.
        let mut stale = custom("m1");
        stale["settings"]["apiKey"] = json!("stale");
        let config = switch_at((&p, &m), "ccs-a", stale, Ok).unwrap();
        assert_eq!(config["settings"]["apiKey"], "sk-test");
        // A provider removed from Cline is written back from the archive.
        let config = switch_at((&p, &m), "ccs-b", custom("m2"), Ok).unwrap();
        assert_eq!(config["provider"]["name"], "Relay");
        assert_eq!(docs(&dir).providers["lastUsedProvider"], "ccs-b");
    }

    #[test]
    fn add_keeps_the_current_provider_and_never_overwrites_a_live_entry() {
        let dir = TempDir::new().unwrap();
        let (p, m) = paths(&dir);
        save(&dir, "ccs-a", Some(custom("m1")), true).unwrap();
        let failed = add_at((&p, &m), "ccs-b", custom("m2"), || {
            Err::<(), _>(AppError::Message("database failed".into()))
        });
        assert!(failed.is_err());
        assert!(docs(&dir).providers["providers"].get("ccs-b").is_none());

        add_at((&p, &m), "ccs-b", custom("m2"), || Ok(())).unwrap();
        let d = docs(&dir);
        assert_eq!(d.providers["lastUsedProvider"], "ccs-a");
        assert_eq!(d.models["providers"]["ccs-b"]["models"]["m2"]["id"], "m2");

        // An entry already in Cline belongs to Cline: adding again keeps it.
        let before = (fs::read(&p).unwrap(), fs::read(&m).unwrap());
        let mut stale = custom("m2");
        stale["settings"]["apiKey"] = json!("stale");
        add_at((&p, &m), "ccs-b", stale, || Ok(())).unwrap();
        assert_eq!((fs::read(&p).unwrap(), fs::read(&m).unwrap()), before);
    }

    #[test]
    fn failed_commit_restores_both_files() {
        let dir = TempDir::new().unwrap();
        let (p, m) = paths(&dir);
        save(&dir, "ccs-a", Some(custom("m1")), true).unwrap();
        let before = (fs::read(&p).unwrap(), fs::read(&m).unwrap());
        let result: Result<(), AppError> =
            save_at((&p, &m), "ccs-b", Some(custom("m2")), true, || {
                Err(AppError::Message("db failed".into()))
            });
        assert!(result.is_err());
        assert_eq!((fs::read(&p).unwrap(), fs::read(&m).unwrap()), before);
    }

    #[test]
    fn invalid_native_json_is_reported_not_overwritten() {
        let dir = TempDir::new().unwrap();
        let (p, m) = paths(&dir);
        fs::write(&p, "{not json").unwrap();
        assert!(save(&dir, "ccs-a", Some(custom("m1")), true).is_err());
        assert_eq!(fs::read_to_string(&p).unwrap(), "{not json");
        assert!(!m.exists());
    }
}
