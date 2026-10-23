//! TRAE CLI 2.0 (`traecli`) keeps Codex-style providers in `~/.trae/traecli.toml`
//! (`TRAE_HOME` overrides the directory). Each CCS provider owns one
//! `[model_providers.<id>]` table; the top-level `model_provider` / `model` pair
//! selects the active one. Without `model_provider`, traecli uses the TRAE
//! account login, represented here by the reserved official provider.
//!
//! A saved CCS provider is `{ provider: <model_providers table>, model: "<id>" }`,
//! or `{ official: true }` for the TRAE login.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
pub(crate) const OFFICIAL_PROVIDER_ID: &str = "trae-official";
/// Provider ids built into traecli (inherited from Codex). Redefining them in
/// `[model_providers]` is ignored or rejected, so CCS never writes these keys.
const RESERVED_PROVIDER_IDS: [&str; 4] = ["openai", "oss", "ollama", "lmstudio"];
const SELECTION_KEYS: [&str; 2] = ["model_provider", "model"];

pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("TRAE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".trae"))
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("traecli.toml")
}

/// Provider and MCP edits share one file, so they share one lock.
pub(crate) fn lock() -> Result<MutexGuard<'static, ()>, AppError> {
    WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))
}

pub(crate) fn is_official(config: &Value) -> bool {
    config.get("official").and_then(Value::as_bool) == Some(true)
}

pub(crate) fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(error) => return Err(AppError::io(path, error)),
    };
    let value: Value = toml::from_str(&text)
        .map_err(|_| AppError::Config("Invalid TRAE CLI TOML configuration".into()))?;
    if value
        .get("model_providers")
        .is_some_and(|providers| !providers.is_object())
    {
        return Err(AppError::Config(
            "TRAE CLI model_providers must be a table".into(),
        ));
    }
    Ok(value)
}

fn providers_in(document: &Value) -> IndexMap<String, Value> {
    let active = document.get("model_provider").and_then(Value::as_str);
    document
        .get("model_providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(id, provider)| {
            provider.is_object() && !RESERVED_PROVIDER_IDS.contains(&id.as_str())
        })
        .map(|(id, provider)| {
            let mut config = json!({ "provider": provider });
            // Only the active provider's model lives in traecli.toml; other
            // providers keep their model in the CCS database (see `merge_saved`).
            if active == Some(id.as_str()) {
                if let Some(model) = document.get("model").filter(|model| model.is_string()) {
                    config["model"] = model.clone();
                }
            }
            (id.clone(), config)
        })
        .collect()
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    Ok(providers_in(&read(&config_path())?))
}

pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    Ok(read(&config_path())?
        .get("model_providers")
        .and_then(|providers| providers.get(id))
        .is_some())
}

/// Native tables win, except for the model of inactive providers, which only
/// the database knows.
pub(crate) fn merge_saved(saved: &Value, native: &Value) -> Value {
    let mut merged = native.clone();
    if merged.get("model").is_none() {
        if let Some(model) = saved.get("model").filter(|model| model.is_string()) {
            merged["model"] = model.clone();
        }
    }
    merged
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if is_official(config) {
        return if id == OFFICIAL_PROVIDER_ID {
            Ok(())
        } else {
            Err(AppError::InvalidInput(
                "Only the TRAE official provider can use the account login".into(),
            ))
        };
    }
    if id.is_empty()
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
    {
        return Err(AppError::InvalidInput(
            "TRAE CLI provider keys may only contain letters, digits, '-' and '_'".into(),
        ));
    }
    if id == OFFICIAL_PROVIDER_ID || RESERVED_PROVIDER_IDS.contains(&id) {
        return Err(AppError::InvalidInput(format!(
            "'{id}' is reserved by TRAE CLI; choose another provider key"
        )));
    }
    let provider = config
        .get("provider")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("Missing TRAE CLI provider settings".into()))?;
    if !provider
        .get("base_url")
        .and_then(Value::as_str)
        .is_some_and(|base| {
            url::Url::parse(base).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
            })
        })
    {
        return Err(AppError::InvalidInput(
            "Enter a valid TRAE CLI endpoint URL".into(),
        ));
    }
    // traecli 0.211 rejects `wire_api = "chat"`; only the Responses API is supported.
    if provider
        .get("wire_api")
        .is_some_and(|wire_api| wire_api.as_str() != Some("responses"))
    {
        return Err(AppError::InvalidInput(
            "TRAE CLI only supports the OpenAI Responses API (wire_api = \"responses\")".into(),
        ));
    }
    for key in ["name", "env_key", "experimental_bearer_token"] {
        if provider.get(key).is_some_and(|value| !value.is_string()) {
            return Err(AppError::InvalidInput(format!(
                "TRAE CLI provider field '{key}' must be a string"
            )));
        }
    }
    if config
        .get("model")
        .and_then(Value::as_str)
        .is_none_or(|model| model.trim().is_empty())
    {
        return Err(AppError::InvalidInput(
            "Enter the model TRAE CLI should use with this provider".into(),
        ));
    }
    Ok(())
}

/// The table written to `[model_providers.<id>]`. traecli requires `name`.
fn native_table(id: &str, config: &Value) -> Value {
    let mut table = config["provider"].clone();
    if table
        .get("name")
        .and_then(Value::as_str)
        .is_none_or(|name| name.trim().is_empty())
    {
        table["name"] = json!(id);
    }
    if table.get("wire_api").is_none() {
        table["wire_api"] = json!("responses");
    }
    table
}

fn json_to_item(value: &Value) -> Result<toml_edit::Item, AppError> {
    let text =
        toml::to_string(&json!({ "v": value })).map_err(|e| AppError::Config(e.to_string()))?;
    let document = text
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| AppError::Config(e.to_string()))?;
    Ok(document["v"].clone())
}

/// Rewrite only the edited provider tables and the selection keys, preserving
/// comments, MCP servers and every other traecli setting.
fn write(path: &Path, before: &Value, after: &Value) -> Result<(), AppError> {
    let original = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(AppError::io(path, error)),
    };
    let mut document = original
        .parse::<toml_edit::DocumentMut>()
        .map_err(|_| AppError::Config("Invalid TRAE CLI TOML configuration".into()))?;
    let old = before.get("model_providers").and_then(Value::as_object);
    let new = after.get("model_providers").and_then(Value::as_object);
    if old != new {
        if document
            .get("model_providers")
            .and_then(toml_edit::Item::as_table_like)
            .is_none()
        {
            let mut table = toml_edit::Table::new();
            table.set_implicit(true);
            document["model_providers"] = toml_edit::Item::Table(table);
        }
        let table = document["model_providers"]
            .as_table_like_mut()
            .ok_or_else(|| AppError::Config("Invalid TRAE CLI model_providers table".into()))?;
        for (key, _) in old.into_iter().flatten() {
            if new.is_none_or(|map| !map.contains_key(key)) {
                table.remove(key);
            }
        }
        for (key, value) in new.into_iter().flatten() {
            if old.and_then(|map| map.get(key)) != Some(value) {
                let mut item = json_to_item(value)?;
                if let Some(table) = item.as_table_mut() {
                    // Keep a blank line between provider tables, as traecli writes them.
                    table.decor_mut().set_prefix("\n");
                }
                table.insert(key, item);
            }
        }
    }
    for key in SELECTION_KEYS {
        if before.get(key) == after.get(key) {
            continue;
        }
        match after.get(key) {
            Some(value) => {
                // Top-level keys must stay above the first table header.
                document[key] = json_to_item(value)?;
            }
            None => {
                document.remove(key);
            }
        }
    }
    atomic_write_private(path, document.to_string().as_bytes())
}

fn apply_provider(
    before: &Value,
    id: &str,
    config: Option<&Value>,
    create: bool,
) -> Result<Value, AppError> {
    if let Some(config) = config {
        validate_provider(id, config)?;
        if is_official(config) {
            return Ok(before.clone());
        }
    } else if id == OFFICIAL_PROVIDER_ID {
        return Ok(before.clone());
    }
    let exists = before
        .get("model_providers")
        .and_then(|providers| providers.get(id))
        .is_some();
    if create && exists {
        return Err(AppError::InvalidInput(format!(
            "TRAE CLI provider key '{id}' already exists"
        )));
    }
    let active = before.get("model_provider").and_then(Value::as_str) == Some(id);
    let mut document = before.clone();
    match config {
        Some(config) => {
            if document.get("model_providers").is_none() {
                document["model_providers"] = json!({});
            }
            document["model_providers"][id] = native_table(id, config);
            if active {
                document["model"] = config["model"].clone();
            }
        }
        None => {
            if !exists {
                return Ok(before.clone());
            }
            if active {
                return Err(AppError::InvalidInput(
                    "Switch TRAE CLI to another provider before removing the active one".into(),
                ));
            }
            document["model_providers"]
                .as_object_mut()
                .unwrap()
                .remove(id);
        }
    }
    Ok(document)
}

fn update(path: &Path, id: &str, config: Option<&Value>, create: bool) -> Result<(), AppError> {
    let before = read(path)?;
    let after = apply_provider(&before, id, config, create)?;
    if before == after {
        return Ok(());
    }
    write(path, &before, &after)
}

pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = lock()?;
    let path = config_path();
    crate::mcode_config::write_and_commit(
        &path,
        || update(&path, id, config.as_ref(), create),
        commit,
    )
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}

pub(crate) fn current_provider() -> Result<String, AppError> {
    Ok(current_provider_in(&read(&config_path())?))
}

fn current_provider_in(document: &Value) -> String {
    match document.get("model_provider").and_then(Value::as_str) {
        Some(id)
            if document
                .get("model_providers")
                .and_then(|providers| providers.get(id))
                .is_some() =>
        {
            id.into()
        }
        // Built-in providers (e.g. `openai`) are not CCS cards.
        Some(_) => String::new(),
        None => OFFICIAL_PROVIDER_ID.into(),
    }
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
    let before = read(path)?;
    let mut after = before.clone();
    if id == OFFICIAL_PROVIDER_ID {
        for key in SELECTION_KEYS {
            if let Some(object) = after.as_object_mut() {
                object.remove(key);
            }
        }
        return crate::mcode_config::write_and_commit(
            path,
            || write(path, &before, &after),
            || commit(json!({ "official": true })),
        );
    }
    let live = before
        .get("model_providers")
        .and_then(|providers| providers.get(id))
        .cloned();
    // A live table belongs to traecli: a card click must not overwrite edits the
    // user made there since the last refresh. Only the model comes from CCS.
    let table = match live {
        Some(table) => table,
        None => {
            validate_provider(id, &archived_config)?;
            native_table(id, &archived_config)
        }
    };
    let model = archived_config
        .get("model")
        .and_then(Value::as_str)
        .filter(|model| !model.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            (before.get("model_provider").and_then(Value::as_str) == Some(id))
                .then(|| {
                    before
                        .get("model")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .flatten()
        })
        .ok_or_else(|| {
            AppError::InvalidInput("Set a model for this TRAE CLI provider first".into())
        })?;
    if after.get("model_providers").is_none() {
        after["model_providers"] = json!({});
    }
    after["model_providers"][id] = table.clone();
    after["model_provider"] = json!(id);
    after["model"] = json!(model);
    let config = json!({ "provider": table, "model": model });
    crate::mcode_config::write_and_commit(path, || write(path, &before, &after), || commit(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(model: &str) -> Value {
        json!({
            "provider": {
                "name": "Example",
                "base_url": "https://example.com/v1",
                "wire_api": "responses",
                "experimental_bearer_token": "sk-test"
            },
            "model": model
        })
    }

    const NATIVE: &str = "# keep this comment\napproval_policy = \"on-request\"\n\n[projects.\"/tmp/demo\"]\ntrust_level = \"trusted\"\n\n[mcp_servers.docs]\ncommand = \"node\"\nargs = [\"server.js\"]\n";

    #[test]
    fn add_preserves_comments_mcp_and_unrelated_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        fs::write(&path, NATIVE).unwrap();
        update(&path, "custom", Some(&node("gpt-5")), true).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("# keep this comment"));
        let value = read(&path).unwrap();
        assert_eq!(value["approval_policy"], "on-request");
        assert_eq!(value["mcp_servers"]["docs"]["command"], "node");
        assert_eq!(
            value["model_providers"]["custom"]["base_url"],
            "https://example.com/v1"
        );
        // Adding a provider does not switch to it.
        assert!(value.get("model_provider").is_none());
        assert!(value.get("model").is_none());
        assert!(update(&path, "custom", Some(&node("gpt-5")), true).is_err());
        update(&path, "custom", None, false).unwrap();
        assert_eq!(
            read(&path).unwrap(),
            toml::from_str::<Value>(NATIVE).unwrap()
        );
    }

    #[test]
    fn switch_writes_selection_and_official_restores_login() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        fs::write(&path, NATIVE).unwrap();
        let config = switch_at(&path, "custom", node("gpt-5"), Ok).unwrap();
        assert_eq!(config["model"], "gpt-5");
        let value = read(&path).unwrap();
        assert_eq!(value["model_provider"], "custom");
        assert_eq!(value["model"], "gpt-5");
        assert_eq!(current_provider_in(&value), "custom");
        assert_eq!(value["model_providers"]["custom"]["name"], "Example");
        let text = fs::read_to_string(&path).unwrap();
        // Top-level selection must be parsed as top-level, not inside a table.
        assert!(text.find("model_provider =").unwrap() < text.find("[projects").unwrap());
        assert!(update(&path, "custom", None, false).is_err());

        let official = switch_at(&path, OFFICIAL_PROVIDER_ID, json!({}), Ok).unwrap();
        assert!(is_official(&official));
        let value = read(&path).unwrap();
        assert!(value.get("model_provider").is_none());
        assert!(value.get("model").is_none());
        assert!(value["model_providers"].get("custom").is_some());
        assert_eq!(current_provider_in(&value), OFFICIAL_PROVIDER_ID);
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("# keep this comment"));
    }

    #[test]
    fn switch_keeps_live_table_and_uses_saved_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        fs::write(
            &path,
            "[model_providers.custom]\nname = \"Native\"\nbase_url = \"https://native.example/v1\"\nenv_key = \"MY_KEY\"\n",
        )
        .unwrap();
        let config = switch_at(&path, "custom", node("model-from-ccs"), Ok).unwrap();
        assert_eq!(config["provider"]["base_url"], "https://native.example/v1");
        assert_eq!(config["provider"]["env_key"], "MY_KEY");
        assert_eq!(read(&path).unwrap()["model"], "model-from-ccs");
        let mut no_model = node("");
        no_model["model"] = json!("");
        fs::write(
            &path,
            "[model_providers.other]\nname = \"x\"\nbase_url = \"https://x.example\"\n",
        )
        .unwrap();
        assert!(switch_at(&path, "other", no_model, Ok).is_err());
    }

    #[test]
    fn switch_rolls_back_when_commit_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        fs::write(&path, NATIVE).unwrap();
        let failed = switch_at(&path, "custom", node("gpt-5"), |_| {
            Err::<(), _>(AppError::Message("database failed".into()))
        });
        assert!(failed.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), NATIVE);
    }

    #[test]
    fn editing_the_active_provider_updates_its_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        switch_at(&path, "custom", node("gpt-5"), Ok).unwrap();
        update(&path, "custom", Some(&node("gpt-5.1")), false).unwrap();
        assert_eq!(read(&path).unwrap()["model"], "gpt-5.1");
        let providers = providers_in(&read(&path).unwrap());
        assert_eq!(providers["custom"]["model"], "gpt-5.1");
    }

    #[test]
    fn listing_skips_builtin_ids_and_merges_saved_models() {
        let document: Value = toml::from_str(
            "model_provider = \"a\"\nmodel = \"m-a\"\n[model_providers.a]\nname = \"A\"\nbase_url = \"https://a.example\"\n[model_providers.b]\nname = \"B\"\nbase_url = \"https://b.example\"\n[model_providers.openai]\nname = \"OpenAI\"\n",
        )
        .unwrap();
        let providers = providers_in(&document);
        assert_eq!(providers.keys().collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(providers["a"]["model"], "m-a");
        assert!(providers["b"].get("model").is_none());
        let merged = merge_saved(&node("saved"), &providers["b"]);
        assert_eq!(merged["model"], "saved");
        assert_eq!(merged["provider"]["base_url"], "https://b.example");
    }

    #[test]
    fn validation_rejects_reserved_keys_chat_wire_api_and_missing_models() {
        assert!(validate_provider("openai", &node("m")).is_err());
        assert!(validate_provider(OFFICIAL_PROVIDER_ID, &node("m")).is_err());
        assert!(validate_provider("bad key", &node("m")).is_err());
        let mut chat = node("m");
        chat["provider"]["wire_api"] = json!("chat");
        assert!(validate_provider("custom", &chat).is_err());
        let mut url = node("m");
        url["provider"]["base_url"] = json!("not a url");
        assert!(validate_provider("custom", &url).is_err());
        assert!(validate_provider("custom", &node(" ")).is_err());
        assert!(validate_provider(OFFICIAL_PROVIDER_ID, &json!({"official": true})).is_ok());
        assert!(validate_provider("custom", &json!({"official": true})).is_err());
    }

    #[test]
    fn malformed_files_are_never_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("traecli.toml");
        fs::write(&path, "[model_providers\n").unwrap();
        assert!(update(&path, "custom", Some(&node("m")), true).is_err());
        assert!(switch_at(&path, "custom", node("m"), Ok).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[model_providers\n");
    }
}
