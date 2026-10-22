//! Kimi Code's native providers and models share one TOML configuration.
//! A saved CCS provider contains `{ provider: LLMProvider, models: { key: LLMModel } }`.
use crate::config::{atomic_write_private, get_home_dir};
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
    std::env::var_os("KIMI_CODE_HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| get_home_dir().join(".kimi-code"))
}

pub(crate) fn config_path() -> PathBuf {
    data_dir().join("config.toml")
}

fn read(path: &Path) -> Result<Value, AppError> {
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
    // OAuth remains owned by Kimi; imported references must survive edits unchanged.
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
        for section in ["providers", "models"] {
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
    if let Some(config) = &config {
        validate_provider(id, config)?;
    }
    let before = read(path)?;
    let mut document = before.clone();
    if config.is_none()
        && document
            .get("providers")
            .and_then(|providers| providers.get(id))
            .is_none()
    {
        return Ok(());
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
    write(path, &before, &document)
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

#[cfg(test)]
mod tests {
    use super::*;
    fn node() -> Value {
        json!({"provider":{"type":"openai","base_url":"https://example.com/v1","api_key":"test"},"models":{"test/model.v1":{"model":"model.v1","max_context_size":128000}}})
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
