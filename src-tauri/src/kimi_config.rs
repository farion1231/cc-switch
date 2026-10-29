//! Kimi Code's native providers and models share one TOML configuration.
//! A saved CCS provider contains `{ provider: LLMProvider, models: { key: LLMModel } }`.
use crate::config::{atomic_write_private, get_home_dir};
use crate::error::AppError;
use crate::provider::Provider;
use indexmap::IndexMap;
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Mutex,
};

static WRITE_LOCK: Mutex<()> = Mutex::new(());
pub(crate) const OFFICIAL_PROVIDER_ID: &str = "managed:kimi-code";
const OFFICIAL_PROVIDER_NAME: &str = "Kimi Official";

pub(crate) fn is_official_provider(config: &Value) -> bool {
    config.pointer("/provider/type").and_then(Value::as_str) == Some("kimi")
        && config
            .pointer("/provider/oauth/key")
            .and_then(Value::as_str)
            .is_some_and(|key| !key.trim().is_empty())
}

pub(crate) fn official_provider_name(id: &str, nickname: Option<&str>) -> String {
    if let Some(nickname) = nickname.map(str::trim).filter(|name| !name.is_empty()) {
        format!("{OFFICIAL_PROVIDER_NAME} · {nickname}")
    } else if id == OFFICIAL_PROVIDER_ID {
        OFFICIAL_PROVIDER_NAME.into()
    } else if let Some(name) = ["Kimi 订阅 ", "Kimi Subscription ", "Kimi "]
        .iter()
        .find_map(|prefix| id.strip_prefix(prefix))
    {
        // Separate accounts are keyed `Kimi 订阅 <nickname>` (see kimi_oauth::new_account).
        format!("{OFFICIAL_PROVIDER_NAME} · {name}")
    } else {
        let account = id.strip_prefix("kimi-code-ccs-").unwrap_or(id);
        format!(
            "{OFFICIAL_PROVIDER_NAME} ({})",
            account.chars().take(8).collect::<String>()
        )
    }
}

/// Fill in native account identities without replacing custom display fields.
/// Names CC Switch generated earlier follow the account's current nickname.
pub(crate) fn normalize_provider_identity(provider: &mut Provider, nickname: Option<&str>) {
    if !is_official_provider(&provider.settings_config) {
        if provider.category.as_deref() == Some("official") {
            provider.category = None;
        }
        return;
    }
    provider.category = Some("official".into());
    if provider.name.trim().is_empty()
        || provider.name == provider.id
        || provider.name.starts_with(OFFICIAL_PROVIDER_NAME)
    {
        provider.name = official_provider_name(&provider.id, nickname);
    }
    if provider
        .icon
        .as_deref()
        .is_none_or(|icon| icon.trim().is_empty())
    {
        provider.icon = Some("kimi".into());
    }
    let global = provider
        .settings_config
        .pointer("/provider/base_url")
        .and_then(Value::as_str)
        .and_then(|base| url::Url::parse(base).ok())
        .is_some_and(|url| url.host_str() == Some("api.kimi.ai"));
    if provider.website_url.as_deref().is_none_or(|url| {
        url.trim().is_empty() || matches!(url, "https://www.kimi.com" | "https://www.kimi.ai")
    }) {
        provider.website_url = Some(
            if global {
                "https://www.kimi.ai"
            } else {
                "https://www.kimi.com"
            }
            .into(),
        );
    }
}

pub(crate) fn data_dir() -> PathBuf {
    std::env::var_os("KIMI_CODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".kimi-code"))
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
        .map_err(|_| AppError::Config("Invalid Kimi Code TOML configuration".into()))?;
    if !value.is_object()
        || ["providers", "models"]
            .iter()
            .any(|key| value.get(key).is_some_and(|v| !v.is_object()))
    {
        return Err(AppError::Config(
            "Kimi Code providers and models must be objects".into(),
        ));
    }
    Ok(value)
}

pub(crate) fn get_providers() -> Result<IndexMap<String, Value>, AppError> {
    let document = read(&config_path())?;
    Ok(document
        .get("providers")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
        .map(|(id, provider)| {
            let models: serde_json::Map<String, Value> = document
                .get("models")
                .and_then(Value::as_object)
                .into_iter()
                .flatten()
                .filter(|(_, model)| model.get("provider").and_then(Value::as_str) == Some(id))
                .map(|(key, model)| (key.clone(), model.clone()))
                .collect();
            (id.clone(), json!({"provider": provider, "models": models}))
        })
        .collect())
}

pub(crate) fn provider_key_exists(id: &str) -> Result<bool, AppError> {
    Ok(read(&config_path())?
        .get("providers")
        .and_then(|p| p.get(id))
        .is_some())
}

pub(crate) fn validate_provider(id: &str, config: &Value) -> Result<(), AppError> {
    if id.trim().is_empty() || id.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Enter a Kimi Code provider key".into(),
        ));
    }
    let provider = &config["provider"];
    if !matches!(
        provider.get("type").and_then(Value::as_str),
        Some("kimi" | "openai" | "openai_responses" | "anthropic" | "google-genai" | "vertexai")
    ) {
        return Err(AppError::InvalidInput(
            "Select a supported Kimi Code provider type".into(),
        ));
    }
    if provider.get("base_url").is_some_and(|base| {
        !base.as_str().is_some_and(|base| {
            url::Url::parse(base).is_ok_and(|url| {
                matches!(url.scheme(), "http" | "https") && url.host_str().is_some()
            })
        })
    }) {
        return Err(AppError::InvalidInput(
            "Enter a valid Kimi Code endpoint URL".into(),
        ));
    }
    // Kimi refreshes native OAuth credentials; references must survive provider edits.
    if provider
        .get("api_key")
        .and_then(Value::as_str)
        .is_none_or(|key| key.trim().is_empty())
        && provider
            .pointer("/oauth/key")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        && provider
            .get("api_key_env")
            .and_then(Value::as_str)
            .is_none_or(|value| value.trim().is_empty())
        && provider
            .get("env")
            .and_then(Value::as_object)
            .is_none_or(|env| {
                ![
                    "KIMI_API_KEY",
                    "ANTHROPIC_API_KEY",
                    "OPENAI_API_KEY",
                    "GOOGLE_API_KEY",
                    "VERTEXAI_API_KEY",
                ]
                .iter()
                .any(|key| {
                    env.get(*key)
                        .and_then(Value::as_str)
                        .is_some_and(|value| !value.trim().is_empty())
                })
            })
    {
        return Err(AppError::InvalidInput(
            "Enter an API key or keep the existing Kimi OAuth reference".into(),
        ));
    }
    if provider.get("api_key_env").is_some()
        && (provider.get("oauth").is_some()
            || provider
                .get("api_key")
                .and_then(Value::as_str)
                .is_some_and(|key| !key.is_empty()))
    {
        return Err(AppError::InvalidInput(
            "api_key_env cannot be combined with api_key or OAuth".into(),
        ));
    }
    let models = config
        .get("models")
        .and_then(Value::as_object)
        .filter(|models| !models.is_empty())
        .ok_or_else(|| AppError::InvalidInput("Add at least one Kimi Code model".into()))?;
    for (key, model) in models {
        if key.trim().is_empty()
            || model
                .get("model")
                .and_then(Value::as_str)
                .is_none_or(|name| name.trim().is_empty())
            || model
                .get("max_context_size")
                .and_then(Value::as_u64)
                .is_none_or(|size| size == 0 || size > i64::MAX as u64)
            || model
                .get("provider")
                .is_some_and(|value| value.as_str() != Some(id))
        {
            return Err(AppError::InvalidInput("Kimi Code models need a key, model ID, positive context size and matching provider".into()));
        }
        // Validate the shape, not Kimi's evolving capability vocabulary. Capabilities
        // outside the editor (such as audio/video) are passed through without changes.
        for pointer in ["/capabilities", "/overrides/capabilities"] {
            if model.pointer(pointer).is_some_and(|caps| {
                caps.as_array()
                    .is_none_or(|caps| caps.iter().any(|cap| !cap.is_string()))
            }) {
                return Err(AppError::InvalidInput(
                    "Invalid Kimi Code model capabilities".into(),
                ));
            }
        }
    }
    Ok(())
}

/// Preserve TOML comments and all tables outside the edited provider's nodes.
fn write(path: &Path, before: &Value, after: &Value) -> Result<(), AppError> {
    let text = {
        let original = fs::read_to_string(path).unwrap_or_default();
        let mut document = original
            .parse::<toml_edit::DocumentMut>()
            .map_err(|_| AppError::Config("Invalid Kimi Code TOML configuration".into()))?;
        let serialized = toml::to_string(after).map_err(|e| AppError::Config(e.to_string()))?;
        let next = serialized
            .parse::<toml_edit::DocumentMut>()
            .map_err(|e| AppError::Config(e.to_string()))?;
        for section in ["providers", "models", "services"] {
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
                .ok_or_else(|| AppError::Config("Invalid Kimi Code table".into()))?;
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
        if before.get("default_model") != after.get("default_model") {
            document["default_model"] = next["default_model"].clone();
        }
        document.to_string()
    };
    atomic_write_private(path, text.as_bytes())
}

fn update(path: &Path, id: &str, config: Option<Value>, create: bool) -> Result<(), AppError> {
    let before = read(path)?;
    let after = apply_provider(&before, id, config, create)?;
    if before == after {
        return Ok(());
    }
    write(path, &before, &after)
}

fn apply_provider(
    before: &Value,
    id: &str,
    config: Option<Value>,
    create: bool,
) -> Result<Value, AppError> {
    // An official node is created by signing in. Edits may change its models,
    // but the connection (endpoint and login reference) stays as signed in.
    // Removing it keeps the credential, so the card can be added back.
    let official = before
        .get("providers")
        .and_then(|providers| providers.get(id))
        .filter(|provider| is_official_provider(&json!({"provider": provider})))
        .cloned();
    let config = match (&official, config) {
        (Some(node), Some(mut config)) => {
            config["provider"] = node.clone();
            Some(config)
        }
        (_, config) => config,
    };
    let removing = config.is_none();
    let mut after = apply_provider_unchecked(before, id, config, create)?;
    if official.is_some() && removing {
        repoint_services(before, &mut after, id);
    }
    Ok(after)
}

/// [`apply_provider`] without pinning an official node's connection; only the
/// account manager writes official nodes this way.
fn apply_provider_unchecked(
    before: &Value,
    id: &str,
    config: Option<Value>,
    create: bool,
) -> Result<Value, AppError> {
    if let Some(config) = &config {
        validate_provider(id, config)?;
    }
    let mut document = before.clone();
    if config.is_none()
        && document
            .get("providers")
            .and_then(|providers| providers.get(id))
            .is_none()
    {
        return Ok(before.clone());
    }
    if create && document.get("providers").and_then(|p| p.get(id)).is_some() {
        return Err(AppError::InvalidInput(format!(
            "Kimi Code provider key '{id}' already exists"
        )));
    }
    let selected = document
        .get("default_model")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let models = document.get("models").and_then(Value::as_object);
    if models
        .and_then(|models| models.get(selected))
        .and_then(|model| model.get("provider"))
        .and_then(Value::as_str)
        == Some(id)
        && config
            .as_ref()
            .and_then(|config| config["models"].get(selected))
            .is_none()
    {
        return Err(AppError::InvalidInput(
            "Select another default model in Kimi Code before removing this model or provider"
                .into(),
        ));
    }
    let removed_model = |key: &str| {
        models
            .and_then(|models| models.get(key))
            .and_then(|model| model.get("provider"))
            .and_then(Value::as_str)
            == Some(id)
            && config
                .as_ref()
                .and_then(|config| config["models"].get(key))
                .is_none()
    };
    if document
        .pointer("/secondary_model/default_model")
        .and_then(Value::as_str)
        .is_some_and(removed_model)
        || document
            .pointer("/secondary_model/models")
            .and_then(Value::as_object)
            .is_some_and(|models| models.keys().any(|key| removed_model(key)))
    {
        return Err(AppError::InvalidInput("Update the secondary model selection in Kimi Code before removing this model or provider".into()));
    }
    if let Some(config) = &config {
        for (key, _) in config["models"].as_object().unwrap() {
            if models
                .and_then(|models| models.get(key))
                .is_some_and(|model| model.get("provider").and_then(Value::as_str) != Some(id))
            {
                return Err(AppError::InvalidInput(format!(
                    "Kimi Code model key '{key}' belongs to another provider"
                )));
            }
        }
    }
    for key in ["providers", "models"] {
        if document.get(key).is_none() {
            document[key] = json!({});
        }
    }
    document["models"]
        .as_object_mut()
        .unwrap()
        .retain(|_, model| model.get("provider").and_then(Value::as_str) != Some(id));
    if let Some(config) = config {
        document["providers"][id] = config["provider"].clone();
        let new_models = config["models"].as_object().unwrap();
        for (key, model) in new_models {
            let mut model = model.clone();
            model["provider"] = json!(id);
            document["models"][key] = model;
        }
        if document
            .get("default_model")
            .and_then(Value::as_str)
            .is_none_or(str::is_empty)
        {
            document["default_model"] = json!(new_models.keys().next().unwrap());
        }
    } else {
        document["providers"].as_object_mut().unwrap().remove(id);
    }
    Ok(document)
}

pub(crate) fn save_and_commit<T>(
    id: &str,
    config: Option<Value>,
    create: bool,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = config_path();
    crate::mcode_config::write_and_commit(&path, || update(&path, id, config, create), commit)
}

pub(crate) fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    save_and_commit(id, Some(config), false, || Ok(()))
}

pub(crate) fn remove_provider(id: &str) -> Result<(), AppError> {
    save_and_commit(id, None, false, || Ok(()))
}

/// Resolve the selected native alias through models, not by splitting its name:
/// aliases can be arbitrary and provider ids may contain punctuation.
pub(crate) fn current_provider() -> Result<String, AppError> {
    let document = read(&config_path())?;
    Ok(current_provider_in(&document).unwrap_or_default().into())
}

fn current_provider_in(document: &Value) -> Option<&str> {
    let selected = document.get("default_model")?.as_str()?;
    let id = document
        .get("models")?
        .get(selected)?
        .get("provider")?
        .as_str()?;
    document.get("providers")?.get(id)?;
    Some(id)
}

fn select_provider_model(document: &mut Value, id: &str) -> Result<(), AppError> {
    let models = document
        .get("models")
        .and_then(Value::as_object)
        .ok_or_else(|| AppError::InvalidInput("The Kimi Code provider has no models".into()))?;
    let belongs_to_provider = |key: &str| {
        models
            .get(key)
            .and_then(|model| model.get("provider"))
            .and_then(Value::as_str)
            == Some(id)
    };
    let selected = document
        .get("default_model")
        .and_then(Value::as_str)
        .filter(|key| belongs_to_provider(key))
        .or_else(|| {
            models
                .keys()
                .find(|key| belongs_to_provider(key))
                .map(String::as_str)
        })
        .ok_or_else(|| AppError::InvalidInput("The Kimi Code provider has no models".into()))?
        .to_string();
    document["default_model"] = json!(selected);
    Ok(())
}

pub(crate) fn switch_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = config_path();
    switch_at(&path, id, archived_config, commit)
}

fn switch_at<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce(Value) -> Result<T, AppError>,
) -> Result<T, AppError> {
    let before = read(path)?;
    // Already-live providers belong to Kimi. A card click must not overwrite
    // changes Kimi or the user made since the last supplier-list refresh.
    let mut after = if before
        .get("providers")
        .and_then(|providers| providers.get(id))
        .is_some()
    {
        before.clone()
    } else {
        apply_provider(&before, id, Some(archived_config), false)?
    };
    select_provider_model(&mut after, id)?;
    let models: serde_json::Map<String, Value> = after["models"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(_, model)| model.get("provider").and_then(Value::as_str) == Some(id))
        .map(|(key, model)| (key.clone(), model.clone()))
        .collect();
    let config = json!({"provider": after["providers"][id], "models": models});
    crate::mcode_config::write_and_commit(path, || write(path, &before, &after), || commit(config))
}

/// "Add" writes a saved provider into config.toml without selecting it, so its
/// models show up in Kimi's /model list. A node already there belongs to Kimi
/// and is kept as is (same rule as switching).
pub(crate) fn add_and_commit<T>(
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = config_path();
    add_at(&path, id, archived_config, commit)
}

fn add_at<T>(
    path: &Path,
    id: &str,
    archived_config: Value,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let before = read(path)?;
    let after = if before
        .get("providers")
        .and_then(|providers| providers.get(id))
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

/// Selecting the account is explicit: signing in alone preserves the current
/// model, while "Use this account" selects one of its official model aliases.
pub(crate) fn use_oauth_account(home: &Path, id: &str) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = home.join("config.toml");
    let before = read(&path)?;
    if before["providers"][id]
        .pointer("/oauth/key")
        .and_then(Value::as_str)
        .is_none_or(str::is_empty)
    {
        return Err(AppError::InvalidInput(
            "Sign in to Kimi Code to add official models first".into(),
        ));
    }
    let mut after = before.clone();
    select_provider_model(&mut after, id)?;
    write(&path, &before, &after)
}

/// Delete an official account's provider node and model aliases. The current
/// account cannot be removed. `commit` runs after the config is written (it
/// deletes the credential and the saved profile); on failure the config is
/// restored byte for byte.
pub(crate) fn remove_oauth_account<T>(
    home: &Path,
    id: &str,
    commit: impl FnOnce() -> Result<T, AppError>,
) -> Result<T, AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = home.join("config.toml");
    let before = read(&path)?;
    if current_provider_in(&before) == Some(id) {
        return Err(AppError::localized(
            "kimi.account.remove_current",
            "这个账号正在使用，请先切换到其他账号再删除",
            "This account is in use; switch to another account before removing it",
        ));
    }
    let mut after = apply_provider_unchecked(&before, id, None, false)?;
    repoint_services(&before, &mut after, id);
    crate::mcode_config::write_and_commit(&path, || write(&path, &before, &after), commit)
}

/// Services (web search / fetch) left pointing at a removed account's login
/// would reference a deleted credential; hand them to Kimi's own default login.
fn repoint_services(before: &Value, after: &mut Value, id: &str) {
    let Some(removed) = before["providers"][id]["oauth"]["key"].as_str() else {
        return;
    };
    let fallback = &before["providers"][OFFICIAL_PROVIDER_ID]["oauth"];
    if id == OFFICIAL_PROVIDER_ID || !fallback.is_object() {
        return;
    }
    for service in after
        .get_mut("services")
        .and_then(Value::as_object_mut)
        .into_iter()
        .flat_map(|services| services.values_mut())
    {
        if service["oauth"]["key"].as_str() == Some(removed) {
            service["oauth"] = fallback.clone();
        }
    }
}

/// Provision an official login under the same lock as provider edits. Keep user
/// aliases, overrides, unrelated services, comments and the current model choice.
/// If credentials cannot be saved, restore the exact original configuration.
pub(crate) fn save_oauth_login(
    home: &Path,
    provider_id: &str,
    token_path: &Path,
    token: &[u8],
    base_url: &str,
    oauth: Value,
    models: serde_json::Map<String, Value>,
) -> Result<(), AppError> {
    let _guard = WRITE_LOCK
        .lock()
        .map_err(|e| AppError::Message(e.to_string()))?;
    let path = home.join("config.toml");
    let before = read(&path)?;
    let mut after = before.clone();
    for section in ["providers", "models", "services"] {
        if after.get(section).is_none() {
            after[section] = json!({});
        }
        if !after[section].is_object() {
            return Err(AppError::Config(format!(
                "Invalid Kimi Code {section} table"
            )));
        }
    }
    let mut merged_models = serde_json::Map::new();
    for (key, model) in &models {
        let mut merged = after["models"].get(key).cloned().unwrap_or(json!({}));
        if merged.get("provider").is_some_and(|id| id != provider_id) {
            return Err(AppError::InvalidInput(format!(
                "Kimi Code model key '{key}' belongs to another provider"
            )));
        }
        let fields = merged
            .as_object_mut()
            .ok_or_else(|| AppError::Config(format!("Invalid Kimi Code model '{key}'")))?;
        // These fields are owned by the official model catalog. User overrides
        // and fields outside this set survive a subsequent login.
        for field in [
            "capabilities",
            "support_efforts",
            "default_effort",
            "protocol",
            "beta_api",
            "adaptive_thinking",
        ] {
            fields.remove(field);
        }
        fields.extend(model.as_object().unwrap().clone());
        merged_models.insert(key.clone(), merged);
    }
    // Signing in again owns only the connection fields; custom headers, env and
    // any other keys the user added to this node survive.
    let mut provider = after["providers"]
        .get(provider_id)
        .filter(|node| node.is_object())
        .cloned()
        .unwrap_or_else(|| json!({}));
    for (key, value) in [
        ("type", json!("kimi")),
        ("base_url", json!(base_url)),
        ("api_key", json!("")),
        ("oauth", oauth.clone()),
    ] {
        provider[key] = value;
    }
    validate_provider(
        provider_id,
        &json!({"provider": provider, "models": merged_models}),
    )?;
    after["providers"][provider_id] = provider;
    for (key, model) in merged_models {
        after["models"][key] = model;
    }
    if after
        .get("default_model")
        .and_then(Value::as_str)
        .is_none_or(|key| after["models"].get(key).is_none())
    {
        after["default_model"] = json!(models.keys().next().unwrap());
    }
    for (name, suffix) in [("moonshot_search", "search"), ("moonshot_fetch", "fetch")] {
        // Signing in must not switch services away from the selected account.
        if after["services"].get(name).is_none() {
            after["services"][name] = json!({
                "base_url": format!("{base_url}/{suffix}"), "api_key": "", "oauth": oauth
            });
        }
    }
    crate::mcode_config::write_and_commit(
        &path,
        || write(&path, &before, &after),
        || {
            if let Some(directory) = token_path.parent() {
                fs::create_dir_all(directory).map_err(|e| AppError::io(directory, e))?;
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    fs::set_permissions(directory, fs::Permissions::from_mode(0o700))
                        .map_err(|e| AppError::io(directory, e))?;
                }
            }
            atomic_write_private(token_path, token)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn official_card_names_follow_nickname_then_provider_key() {
        assert_eq!(
            official_provider_name("Kimi 订阅 登月者9287", Some("新昵称")),
            "Kimi Official · 新昵称"
        );
        // Without a cached nickname the readable key still names the card.
        for id in [
            "Kimi 订阅 登月者9287",
            "Kimi Subscription 登月者9287",
            "Kimi 登月者9287",
        ] {
            assert_eq!(
                official_provider_name(id, None),
                "Kimi Official · 登月者9287"
            );
        }
        assert_eq!(
            official_provider_name("kimi-code-ccs-fd019f9e", None),
            "Kimi Official (fd019f9e)"
        );
        assert_eq!(
            official_provider_name(OFFICIAL_PROVIDER_ID, None),
            "Kimi Official"
        );
    }

    #[test]
    fn official_identity_repairs_imported_name_icon_and_category_without_renaming_native_keys() {
        let config = json!({"provider":{"type":"kimi","base_url":"https://api.kimi.com/coding/v1","oauth":{"storage":"file","key":"oauth/kimi-code"}},"models":{}});
        let mut provider = Provider::with_id(
            OFFICIAL_PROVIDER_ID.into(),
            OFFICIAL_PROVIDER_ID.into(),
            config.clone(),
            None,
        );
        normalize_provider_identity(&mut provider, None);
        assert_eq!(provider.id, OFFICIAL_PROVIDER_ID);
        assert_eq!(provider.name, "Kimi Official");
        assert_eq!(provider.icon.as_deref(), Some("kimi"));
        assert_eq!(provider.category.as_deref(), Some("official"));
        assert_eq!(
            provider.website_url.as_deref(),
            Some("https://www.kimi.com")
        );
        assert_eq!(provider.settings_config, config);
    }

    #[test]
    fn official_identity_preserves_custom_display_fields_and_follows_global_website() {
        let config = json!({"provider":{"type":"kimi","base_url":"https://api.kimi.ai/coding/v1","oauth":{"storage":"file","key":"oauth/kimi-code-env-0e4f99c69cc27850"}},"models":{}});
        let mut provider = Provider::with_id(
            OFFICIAL_PROVIDER_ID.into(),
            "My Kimi account".into(),
            config,
            Some("https://www.kimi.com".into()),
        );
        provider.icon = Some("user-selected-icon".into());
        provider.notes = Some("Keep this note".into());
        normalize_provider_identity(&mut provider, None);
        assert_eq!(provider.name, "My Kimi account");
        assert_eq!(provider.icon.as_deref(), Some("user-selected-icon"));
        assert_eq!(provider.notes.as_deref(), Some("Keep this note"));
        assert_eq!(provider.website_url.as_deref(), Some("https://www.kimi.ai"));
        provider.website_url = Some("https://my-bookmark.example".into());
        normalize_provider_identity(&mut provider, None);
        assert_eq!(
            provider.website_url.as_deref(),
            Some("https://my-bookmark.example")
        );
        provider.settings_config["provider"]
            .as_object_mut()
            .unwrap()
            .remove("oauth");
        normalize_provider_identity(&mut provider, None);
        assert!(provider.category.is_none());
    }

    fn login_models() -> serde_json::Map<String, Value> {
        serde_json::from_value(json!({"kimi-code/k3": {
            "provider": "managed:kimi-code", "model": "k3", "max_context_size": 1048576,
            "capabilities": ["thinking", "tool_use"]
        }}))
        .unwrap()
    }

    #[test]
    fn oauth_login_preserves_custom_config_and_uses_native_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let credential = dir.path().join("credentials/kimi-code.json");
        let initial = "# preserve this comment\ndefault_model = 'custom/model'\n[providers.custom]\ntype = 'openai'\napi_key = 'test'\n[models.'custom/model']\nprovider = 'custom'\nmodel = 'custom'\nmax_context_size = 32000\n[models.'kimi-code/k3']\nprovider = 'managed:kimi-code'\nmodel = 'k3'\nmax_context_size = 262144\n[models.'kimi-code/k3'.overrides]\ndefault_effort = 'max'\n[providers.'managed:kimi-code']\ntype = 'kimi'\nbase_url = 'https://stale.example/v1'\napi_key = ''\n[providers.'managed:kimi-code'.custom_headers]\nX-Team = 'blue'\n[providers.'managed:kimi-code'.oauth]\nstorage = 'file'\nkey = 'oauth/kimi-code'\n[services.moonshot_search]\nbase_url = 'https://custom.example/search'\napi_key = 'custom-search'\n[thinking]\neffort = 'high'\n";
        fs::write(&path, initial).unwrap();
        let oauth = json!({"storage": "file", "key": "oauth/kimi-code"});
        save_oauth_login(
            dir.path(),
            OFFICIAL_PROVIDER_ID,
            &credential,
            b"test-credential",
            "https://api.kimi.com/coding/v1",
            oauth.clone(),
            login_models(),
        )
        .unwrap();
        let config = read(&path).unwrap();
        assert_eq!(config["default_model"], "custom/model");
        assert_eq!(config["thinking"]["effort"], "high");
        assert_eq!(
            config["models"]["kimi-code/k3"]["overrides"]["default_effort"],
            "max"
        );
        assert_eq!(
            config["models"]["kimi-code/k3"]["max_context_size"],
            1048576
        );
        assert_eq!(config["providers"]["managed:kimi-code"]["oauth"], oauth);
        // Signing in again replaces the connection, not the user's headers.
        assert_eq!(
            config["providers"]["managed:kimi-code"]["custom_headers"]["X-Team"],
            "blue"
        );
        assert_eq!(
            config["providers"]["managed:kimi-code"]["base_url"],
            "https://api.kimi.com/coding/v1"
        );
        assert_eq!(
            config["services"]["moonshot_search"]["api_key"],
            "custom-search"
        );
        assert_eq!(config["services"]["moonshot_fetch"]["oauth"], oauth);
        assert!(fs::read_to_string(path)
            .unwrap()
            .contains("# preserve this comment"));
        assert_eq!(fs::read(credential).unwrap(), b"test-credential");
    }

    #[test]
    fn oauth_login_rolls_back_config_on_credential_write_failure() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let initial = "# original\n[thinking]\nenabled = true\n";
        fs::write(&path, initial).unwrap();
        let blocked = dir.path().join("credentials");
        fs::write(&blocked, "not a directory").unwrap();
        assert!(save_oauth_login(
            dir.path(),
            OFFICIAL_PROVIDER_ID,
            &blocked.join("kimi-code.json"),
            b"new",
            "https://api.kimi.com/coding/v1",
            json!({"storage":"file","key":"oauth/kimi-code"}),
            login_models()
        )
        .is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), initial);
        assert_eq!(fs::read_to_string(blocked).unwrap(), "not a directory");
    }

    #[test]
    fn using_oauth_account_selects_official_model_only_after_explicit_selection() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let initial = "# preserve\ndefault_model = 'custom/model'\n[models.'custom/model']\nprovider = 'custom'\nmodel = 'custom'\nmax_context_size = 32000\n[thinking]\neffort = 'high'\n";
        fs::write(&path, initial).unwrap();
        assert!(use_oauth_account(dir.path(), OFFICIAL_PROVIDER_ID).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), initial);
        save_oauth_login(
            dir.path(),
            OFFICIAL_PROVIDER_ID,
            &dir.path().join("credentials/kimi-code.json"),
            b"credential",
            "https://api.kimi.com/coding/v1",
            json!({"storage":"file","key":"oauth/kimi-code"}),
            login_models(),
        )
        .unwrap();
        assert_eq!(read(&path).unwrap()["default_model"], "custom/model");
        use_oauth_account(dir.path(), OFFICIAL_PROVIDER_ID).unwrap();
        let config = read(&path).unwrap();
        assert_eq!(config["default_model"], "kimi-code/k3");
        assert_eq!(config["thinking"]["effort"], "high");
        assert_eq!(config["models"]["custom/model"]["model"], "custom");
        assert!(fs::read_to_string(path).unwrap().contains("# preserve"));
    }

    #[test]
    fn oauth_login_rejects_model_collisions_before_replacing_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let credential = dir.path().join("kimi-code.json");
        let initial = "[models.'kimi-code/k3']\nprovider = 'custom'\nmodel = 'my-model'\nmax_context_size = 1000\n";
        fs::write(&path, initial).unwrap();
        fs::write(&credential, "old").unwrap();
        assert!(save_oauth_login(
            dir.path(),
            OFFICIAL_PROVIDER_ID,
            &credential,
            b"new",
            "https://api.kimi.com/coding/v1",
            json!({"storage":"file","key":"oauth/kimi-code"}),
            login_models()
        )
        .is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), initial);
        assert_eq!(fs::read_to_string(credential).unwrap(), "old");
    }

    fn node() -> Value {
        json!({"provider":{"type":"openai","base_url":"https://example.com/v1","api_key":"test"},"models":{"test/model.v1":{"model":"model.v1","max_context_size":128000}}})
    }

    #[test]
    fn kimi_switch_uses_fresh_native_models_and_preserves_the_selected_alias() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let native = "# preserve\ndefault_model = 'arbitrary-alias'\n[providers.custom]\ntype = 'openai'\nbase_url = 'https://fresh.example/v1'\napi_key = 'native'\n[models.'arbitrary-alias']\nprovider = 'custom'\nmodel = 'fresh-model'\nmax_context_size = 32000\n[models.'arbitrary-alias'.overrides]\ndefault_effort = 'high'\n";
        fs::write(&path, native).unwrap();
        let config = switch_at(&path, "custom", json!({}), Ok).unwrap();
        assert_eq!(config["provider"]["base_url"], "https://fresh.example/v1");
        assert_eq!(
            config["models"]["arbitrary-alias"]["overrides"]["default_effort"],
            "high"
        );
        assert_eq!(read(&path).unwrap()["default_model"], "arbitrary-alias");
        assert_eq!(current_provider_in(&read(&path).unwrap()), Some("custom"));
        assert_eq!(fs::read_to_string(path).unwrap(), native);
    }

    #[test]
    fn kimi_switch_rolls_back_membership_and_default_selection_when_commit_fails() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let native = "# original\ndefault_model = 'old-alias'\n[providers.original]\ntype = 'openai'\napi_key = 'test'\n[models.old-alias]\nprovider = 'original'\nmodel = 'old'\nmax_context_size = 1000\n";
        fs::write(&path, native).unwrap();
        let failed = switch_at(&path, "new", node(), |_| {
            Err::<(), _>(AppError::Message("database failed".into()))
        });
        assert!(failed.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), native);
        switch_at(&path, "new", node(), |_| Ok(())).unwrap();
        let config = read(&path).unwrap();
        assert_eq!(current_provider_in(&config), Some("new"));
        assert_eq!(config["default_model"], "test/model.v1");
        assert!(config["providers"].get("original").is_some());
        assert!(config["models"].get("old-alias").is_some());
        assert!(fs::read_to_string(path).unwrap().contains("# original"));
    }

    #[test]
    fn kimi_add_keeps_the_selected_model_and_never_overwrites_a_live_node() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let native = "# original\ndefault_model = 'old-alias'\n[providers.original]\ntype = 'openai'\napi_key = 'test'\n[models.old-alias]\nprovider = 'original'\nmodel = 'old'\nmax_context_size = 1000\n";
        fs::write(&path, native).unwrap();
        let failed = add_at(&path, "new", node(), || {
            Err::<(), _>(AppError::Message("database failed".into()))
        });
        assert!(failed.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), native);

        add_at(&path, "new", node(), || Ok(())).unwrap();
        let config = read(&path).unwrap();
        assert_eq!(config["default_model"], "old-alias");
        assert_eq!(config["models"]["test/model.v1"]["provider"], "new");
        assert!(fs::read_to_string(&path).unwrap().contains("# original"));

        // 已经在 Kimi 里的节点归 Kimi：再点「添加」不拿存档覆盖
        let before = fs::read_to_string(&path).unwrap();
        let mut stale = node();
        stale["provider"]["base_url"] = json!("https://stale.example/v1");
        add_at(&path, "new", stale, || Ok(())).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), before);
    }

    #[test]
    fn kimi_native_roundtrip_preserves_login_other_models_and_comments() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let initial = "# user settings\ndefault_model = 'official'\ndefault_thinking = true\n[providers.official]\ntype = 'kimi'\nbase_url = 'https://api.kimi.com/coding/v1'\napi_key = ''\n[providers.official.oauth]\nkey = 'kimi-code'\nstorage = 'file'\n[models.official]\nprovider = 'official'\nmodel = 'kimi-for-coding'\nmax_context_size = 262144\n[services.moonshot_search]\nbase_url = 'https://example.com/search'\napi_key = 'search'\n";
        fs::write(&path, initial).unwrap();
        update(&path, "custom", Some(node()), true).unwrap();
        let value = read(&path).unwrap();
        assert_eq!(value["providers"]["official"]["oauth"]["key"], "kimi-code");
        assert_eq!(value["models"]["test/model.v1"]["provider"], "custom");
        assert_eq!(value["default_model"], "official");
        assert!(fs::read_to_string(&path)
            .unwrap()
            .contains("# user settings"));
        update(&path, "custom", None, false).unwrap();
        assert_eq!(
            read(&path).unwrap(),
            toml::from_str::<Value>(initial).unwrap()
        );
    }

    #[test]
    fn kimi_rejects_collisions_default_removal_and_malformed_files_without_writing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        update(&path, "first", Some(node()), true).unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(update(&path, "first", Some(node()), true).is_err());
        assert!(update(&path, "second", Some(node()), true).is_err());
        assert!(update(&path, "first", None, false).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
        fs::write(&path, "[providers\n").unwrap();
        assert!(update(&path, "test", Some(node()), false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "[providers\n");
    }

    #[test]
    fn kimi_model_validation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        update(&path, "test", Some(node()), true).unwrap();
        assert_eq!(read(&path).unwrap()["default_model"], "test/model.v1");
        let mut invalid = node();
        invalid["models"]["test/model.v1"]["max_context_size"] = json!(0);
        assert!(validate_provider("test", &invalid).is_err());
    }

    #[test]
    fn kimi_native_capabilities_survive_unrelated_provider_edits() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = node();
        config["models"]["test/model.v1"]["capabilities"] = json!([
            "image_in",
            "audio_in",
            "thinking",
            "video_in",
            "image_out",
            "audio_out",
            "future_capability",
            "tool_use",
            "dynamically_loaded_tools"
        ]);
        config["models"]["test/model.v1"]["overrides"] = json!({"capabilities":["audio_in", "image_in", "video_in", "future_override", "always_thinking"], "default_effort":"high"});
        validate_provider("test", &config).unwrap();
        update(&path, "test", Some(config.clone()), true).unwrap();
        config["provider"]["base_url"] = json!("https://updated.example.com/v1");
        update(&path, "test", Some(config.clone()), false).unwrap();
        let saved = read(&path).unwrap();
        assert_eq!(
            saved["models"]["test/model.v1"]["capabilities"],
            config["models"]["test/model.v1"]["capabilities"]
        );
        assert_eq!(
            saved["models"]["test/model.v1"]["overrides"],
            config["models"]["test/model.v1"]["overrides"]
        );
        assert_eq!(saved["providers"]["test"], config["provider"]);
    }

    #[test]
    fn kimi_database_commit_failure_restores_original_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let initial = "# local preferences\n[thinking]\neffort='high'\n";
        fs::write(&path, initial).unwrap();
        let result: Result<(), AppError> = crate::mcode_config::write_and_commit(
            &path,
            || update(&path, "test", Some(node()), true),
            || Err(AppError::Database("read only".into())),
        );
        assert!(result.is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), initial);
    }

    #[test]
    fn kimi_preserves_environment_auth_and_protects_secondary_model_references() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut config = node();
        config["provider"]
            .as_object_mut()
            .unwrap()
            .remove("api_key");
        config["provider"]["api_key_env"] = json!("CUSTOM_API_KEY");
        validate_provider("custom", &config).unwrap();
        update(&path, "custom", Some(config), true).unwrap();
        let initial = fs::read_to_string(&path)
            .unwrap()
            .replace("default_model = \"test/model.v1\"", "default_model = \"\"");
        fs::write(
            &path,
            format!("{initial}\n[secondary_model]\ndefault_model='test/model.v1'\n"),
        )
        .unwrap();
        let bytes = fs::read(&path).unwrap();
        assert!(update(&path, "custom", None, false).is_err());
        assert_eq!(fs::read(&path).unwrap(), bytes);
    }
}
