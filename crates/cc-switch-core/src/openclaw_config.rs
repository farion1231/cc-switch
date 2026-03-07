use std::collections::HashMap;
use std::path::PathBuf;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::config::{get_openclaw_config_dir, write_json_file};
use crate::error::AppError;

pub fn get_openclaw_dir() -> PathBuf {
    get_openclaw_config_dir()
}

pub fn get_openclaw_config_path() -> PathBuf {
    get_openclaw_dir().join("openclaw.json")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawProviderConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<OpenClawModelEntry>,
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenClawModelEntry {
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

pub fn read_openclaw_config() -> Result<Value, AppError> {
    let path = get_openclaw_config_path();
    if !path.exists() {
        return Ok(json!({
            "models": {
                "mode": "merge",
                "providers": {}
            }
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content)
        .map_err(|e| AppError::Config(format!("Failed to parse OpenClaw config as JSON5: {e}")))
}

pub fn write_openclaw_config(config: &Value) -> Result<(), AppError> {
    let path = get_openclaw_config_path();
    write_json_file(&path, config)
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_openclaw_config()?;
    Ok(config
        .get("models")
        .and_then(|value| value.get("providers"))
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, provider_config: Value) -> Result<(), AppError> {
    let mut full_config = read_openclaw_config()?;

    if full_config.get("models").is_none() {
        full_config["models"] = json!({
            "mode": "merge",
            "providers": {}
        });
    }

    if full_config["models"].get("providers").is_none() {
        full_config["models"]["providers"] = json!({});
    }

    if let Some(providers) = full_config["models"]
        .get_mut("providers")
        .and_then(|value| value.as_object_mut())
    {
        providers.insert(id.to_string(), provider_config);
    }

    write_openclaw_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_openclaw_config()?;
    if let Some(providers) = config
        .get_mut("models")
        .and_then(|value| value.get_mut("providers"))
        .and_then(|value| value.as_object_mut())
    {
        providers.remove(id);
    }
    write_openclaw_config(&config)
}

pub fn get_typed_providers() -> Result<IndexMap<String, OpenClawProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenClawProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(err) => {
                log::warn!("Failed to parse OpenClaw provider '{id}': {err}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenClawProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn openclaw_provider_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        set_typed_provider(
            "demo",
            &OpenClawProviderConfig {
                base_url: Some("https://example.com/v1".to_string()),
                api_key: Some("key".to_string()),
                api: Some("openai-completions".to_string()),
                models: vec![OpenClawModelEntry {
                    id: "gpt-4.1".to_string(),
                    name: Some("GPT-4.1".to_string()),
                    alias: None,
                    extra: HashMap::new(),
                }],
                extra: HashMap::new(),
            },
        )?;

        let providers = get_typed_providers()?;
        assert_eq!(
            providers
                .get("demo")
                .and_then(|item| item.base_url.as_deref()),
            Some("https://example.com/v1")
        );

        Ok(())
    }
}
