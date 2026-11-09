//! AtomCode TUI 的 live 配置（`~/.atomcode/config.toml`）。
//!
//! AtomCode 以共存模式管理供应商：一个账号对应一个
//! `[provider_accounts.<id>]` 节点，其模型以 `account = "<id>"` 挂在
//! `[models."<key>"]` 下；切换供应商即把 `default_provider` /
//! `default_model` 指到该账号的某个模型键。
use crate::config::get_home_dir;
use crate::error::AppError;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());

pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("ATOMCODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".atomcode"))
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

pub(crate) fn read(path: &Path) -> Result<Value, AppError> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(json!({})),
        Err(error) => return Err(AppError::io(path, error)),
    };
    let value: Value = toml::from_str(&text)
        .map_err(|_| AppError::Config("Invalid AtomCode TOML configuration".into()))?;
    if !value.is_object()
        || ["provider_accounts", "models"]
            .iter()
            .any(|key| value.get(key).is_some_and(|v| !v.is_object()))
    {
        return Err(AppError::Config(
            "AtomCode provider_accounts and models must be objects".into(),
        ));
    }
    Ok(value)
}

fn models_of(document: &Value, account: &str) -> IndexMap<String, Value> {
    document
        .get("models")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, model)| model.get("account").and_then(Value::as_str) == Some(account))
        .map(|(key, model)| (key.clone(), model.clone()))
        .collect()
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read(&config_path())?;
    Ok(document
        .get("provider_accounts")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(id, account)| {
            let models = models_of(&document, id);
            (id.clone(), json!({"account": account, "models": models}))
        })
        .collect())
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() || id.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Enter an AtomCode account key".into(),
        ));
    }
    if !id
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(AppError::InvalidInput(
            "AtomCode account key may only contain letters, digits, '-', '_' or '.'".into(),
        ));
    }
    let account = &config["account"];
    if !matches!(
        account.get("provider").and_then(Value::as_str),
        Some("openai" | "anthropic" | "deepseek" | "gemini" | "openai_responses")
    ) {
        return Err(AppError::InvalidInput(
            "Select a supported AtomCode provider type".into(),
        ));
    }
    let base_url = account
        .get("base_url")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if !url::Url::parse(base_url)
        .is_ok_and(|url| matches!(url.scheme(), "http" | "https") && url.host_str().is_some())
    {
        return Err(AppError::InvalidInput(
            "Enter a valid AtomCode endpoint URL".into(),
        ));
    }
    // API key is optional: an account may rely on AtomCode's native OAuth (`auth.toml`).
    let models = config
        .get("models")
        .and_then(Value::as_object)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Add at least one AtomCode model".into()))?;
    for (key, model) in models {
        if key.trim().is_empty()
            || model
                .get("account")
                .is_some_and(|value| value.as_str() != Some(id))
            || model
                .get("model")
                .and_then(Value::as_str)
                .is_none_or(|name| name.trim().is_empty())
        {
            return Err(AppError::InvalidInput(
                "AtomCode models need a key, model ID and matching account".into(),
            ));
        }
        if let Some(size) = model.get("context_window").and_then(Value::as_u64) {
            if size == 0 {
                return Err(AppError::InvalidInput(
                    "AtomCode model context_window must be positive".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Preserve TOML comments and all tables outside the edited account's nodes.
fn write(path: &Path, before: &Value, after: &Value) -> Result<(), AppError> {
    let text = {
        let original = fs::read_to_string(path).unwrap_or_default();
        let mut document = original
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| AppError::Config("Invalid AtomCode TOML configuration".into()))?;
        let serialized = toml::to_string(after).map_err(|e| AppError::Config(e.to_string()))?;
        let next = serialized
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::Config(e.to_string()))?;
        for section in ["provider_accounts", "models"] {
            let old = before.get(section).and_then(Value::as_object);
            let new = after.get(section).and_then(Value::as_object);
            if old == new {
                continue;
            }
            if document.get(section).is_none() {
                document[section] = toml_edit::Item::Table(toml_edit::Table::new());
            }
            let table = document[section]
                .as_table_like_mut()
                .ok_or_else(|| AppError::Config("Invalid AtomCode table".into()))?;
            for (key, _) in old.into_iter().flatten() {
                if new.is_none_or(|map| !map.contains_key(key)) {
                    table.remove(key);
                }
            }
            for (key, value) in new.into_iter().flatten() {
                if old.and_then(|map| map.get(key)) != Some(value) {
                    table.insert(key, next[section][key].clone());
                }
            }
        }
        for field in ["default_provider", "default_model"] {
            if before.get(field) != after.get(field) {
                document[field] = next[field].clone();
            }
        }
        document.to_string()
    };
    crate::config::atomic_write_private(path, text.as_bytes())
}

fn apply_provider(
    before: &Value,
    id: &str,
    config: Option<Value>,
    create: bool,
) -> Result<Value, AppError> {
    if let Some(config) = &config {
        validate_provider(id, config)?;
    }
    let mut document = before.clone();
    let exists = document
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(id))
        .is_some();
    if config.is_none() && !exists {
        return Ok(before.clone());
    }
    if create && exists {
        return Err(AppError::InvalidInput(format!(
            "AtomCode account key '{id}' already exists"
        )));
    }
    // Removing a provider must not strand the active selection on its models.
    if config.is_none() && selected_model_belongs_to(&document, id) {
        return Err(AppError::InvalidInput(
            "Select another provider in AtomCode before removing this one".into(),
        ));
    }
    for key in ["provider_accounts", "models"] {
        if document.get(key).is_none() {
            document[key] = json!({});
        }
    }
    // New model keys must not collide with models owned by another account.
    if let Some(config) = &config {
        for key in config["models"].as_object().unwrap().keys() {
            if document["models"]
                .as_object()
                .unwrap()
                .get(key)
                .and_then(|model| model.get("account"))
                .and_then(Value::as_str)
                .is_some_and(|account| account != id)
            {
                return Err(AppError::InvalidInput(format!(
                    "AtomCode model key '{key}' belongs to another provider"
                )));
            }
        }
    }
    document["models"]
        .as_object_mut()
        .unwrap()
        .retain(|_, model| model.get("account").and_then(Value::as_str) != Some(id));
    if let Some(config) = config {
        document["provider_accounts"][id] = config["account"].clone();
        for (key, model) in config["models"].as_object().unwrap() {
            let mut model = model.clone();
            model["account"] = json!(id);
            document["models"][key] = model;
        }
    } else {
        document["provider_accounts"]
            .as_object_mut()
            .unwrap()
            .remove(id);
    }
    Ok(document)
}

fn selected_model_belongs_to(document: &Value, id: &str) -> bool {
    document
        .get("default_model")
        .and_then(Value::as_str)
        .and_then(|key| document.get("models").and_then(|models| models.get(key)))
        .and_then(|model| model.get("account"))
        .and_then(Value::as_str)
        == Some(id)
}

pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let path = config_path();
    save_at_and_commit(&path, id, config, create, commit)
}

fn save_at_and_commit<T>(
    path: &Path,
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    crate::mcode_config::write_and_commit(
        path,
        || {
            let before = read(path)?;
            let after = apply_provider(&before, id, config, create)?;
            if before == after {
                return Ok(());
            }
            write(path, &before, &after)
        },
        commit,
    )
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}

fn select_provider_model(document: &mut Value, id: &str) -> Result<(), AppError> {
    let models = document
        .get("models")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("The AtomCode provider has no models".into()))?;
    let belongs_to_account = |key: &str| {
        models
            .get(key)
            .and_then(|model| model.get("account"))
            .and_then(Value::as_str)
            == Some(id)
    };
    let selected = document
        .get("default_model")
        .and_then(Value::as_str)
        .filter(|key| belongs_to_account(key))
        .or_else(|| {
            models
                .keys()
                .find(|key| belongs_to_account(key))
                .map(String::as_str)
        })
        .ok_or_else(|| AppError::InvalidInput("The AtomCode provider has no models".into()))?
        .to_string();
    document["default_model"] = json!(selected);
    document["default_provider"] = json!(selected);
    Ok(())
}

pub(crate) fn switch_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let path = config_path();
    switch_at_and_commit(&path, id, archived_config, commit)
}

fn switch_at_and_commit<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let before = read(path)?;
    // A node already in config.toml belongs to AtomCode; a card click must not
    // overwrite changes made outside CC Switch since the last refresh.
    let mut after = if before
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(id))
        .is_some()
    {
        before.clone()
    } else {
        apply_provider(&before, id, Some(archived_config), false)?
    };
    select_provider_model(&mut after, id)?;
    let models = models_of(&after, id);
    let config = json!({
        "account": after["provider_accounts"][id],
        "models": models,
    });
    crate::mcode_config::write_and_commit(path, || write(path, &before, &after), || commit(config))
}

/// "Add" writes a saved account into config.toml without changing
/// `default_model` ("switch" does that).
pub(crate) fn add_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let path = config_path();
    add_at_and_commit(&path, id, archived_config, commit)
}

fn add_at_and_commit<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let before = read(path)?;
    let after = if before
        .get("provider_accounts")
        .and_then(|accounts| accounts.get(id))
        .is_some()
    {
        before.clone()
    } else {
        apply_provider(&before, id, Some(archived_config), false)?
    };
    crate::mcode_config::write_and_commit(
        path,
        || {
            if before == after {
                Ok(())
            } else {
                write(path, &before, &after)
            }
        },
        commit,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toml_text() -> String {
        r#"
# AtomCode keeps user comments here.
default_provider = "acct-key-a"
default_model = "acct-key-a"

[lsp]
enabled = false

[provider_accounts.acct]
provider = "openai"
base_url = "https://api.example.com/v1"
api_key = "sk-test"

[models."acct-key-a"]
account = "acct"
model = "key-a"
context_window = 262144
"#
        .to_string()
    }

    #[test]
    fn switch_preserves_comments_and_unrelated_tables() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        let config = json!({
            "account": {"provider": "deepseek", "base_url": "https://api.deepseek.example/v1", "api_key": "sk-2"},
            "models": {"fresh-ds": {"model": "deepseek-chat", "context_window": 65536}}
        });
        switch_at_and_commit(&path, "fresh", config.clone(), |written| {
            assert_eq!(written["account"]["provider"], json!("deepseek"));
            Ok(())
        })
        .unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.contains("AtomCode keeps user comments here."));
        assert!(text.contains("[lsp]"));
        let document = read(&path).unwrap();
        assert_eq!(document["default_model"], json!("fresh-ds"));
        assert_eq!(document["default_provider"], json!("fresh-ds"));
        // 未被切换的既有账号原样保留。
        assert_eq!(
            document["provider_accounts"]["acct"]["provider"],
            json!("openai")
        );
        assert_eq!(document["models"]["acct-key-a"]["model"], json!("key-a"));
    }

    #[test]
    fn add_writes_account_without_changing_default_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        let config = json!({
            "account": {"provider": "openai", "base_url": "https://b.example/v1"},
            "models": {"b-m": {"model": "m"}}
        });
        add_at_and_commit(&path, "b", config, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert_eq!(document["default_model"], json!("acct-key-a"));
        assert_eq!(
            document["provider_accounts"]["b"]["base_url"],
            json!("https://b.example/v1")
        );
    }

    #[test]
    fn remove_rejects_current_account_and_preserves_others() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        assert!(save_at_and_commit(&path, "acct", None, false, || Ok(())).is_err());
        let config = json!({
            "account": {"provider": "openai", "base_url": "https://b.example/v1"},
            "models": {"b-m": {"model": "m"}}
        });
        switch_at_and_commit(&path, "b", config, |_| Ok(())).unwrap();
        save_at_and_commit(&path, "acct", None, false, || Ok(())).unwrap();
        let document = read(&path).unwrap();
        assert!(document["provider_accounts"].get("acct").is_none());
        assert_eq!(document["default_model"], json!("b-m"));
    }

    #[test]
    fn set_provider_rejects_collisions_and_bad_models() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, toml_text()).unwrap();
        let config = json!({
            "account": {"provider": "openai", "base_url": "https://x.example/v1"},
            "models": {"acct-key-a": {"model": "m"}}
        });
        assert!(save_at_and_commit(&path, "x", Some(config.clone()), false, || Ok(())).is_err());
        let bad = json!({
            "account": {"provider": "openai", "base_url": "not a url"},
            "models": {"m": {"model": "m"}}
        });
        assert!(save_at_and_commit(&path, "x", Some(bad), false, || Ok(())).is_err());
        let document = read(&path).unwrap();
        assert!(document["provider_accounts"].get("x").is_none());
    }
}
