use std::path::PathBuf;

use indexmap::IndexMap;
use serde_json::{json, Map, Value};

use crate::config::{get_opencode_config_dir, write_json_file};
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;

pub fn get_opencode_dir() -> PathBuf {
    get_opencode_config_dir()
}

pub fn get_opencode_config_path() -> PathBuf {
    get_opencode_dir().join("opencode.json")
}

#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();
    if !path.exists() {
        return Ok(json!({ "$schema": "https://opencode.ai/config.json" }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(&path, e))
}

pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();
    write_json_file(&path, config)
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|value| value.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    if let Some(providers) = config
        .get_mut("provider")
        .and_then(|value| value.as_object_mut())
    {
        providers.remove(id);
    }
    write_opencode_config(&config)
}

pub fn get_typed_providers() -> Result<IndexMap<String, OpenCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<OpenCodeProviderConfig>(value.clone()) {
            Ok(config) => {
                result.insert(id, config);
            }
            Err(err) => {
                log::warn!("Failed to parse provider '{id}': {err}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|value| value.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config
        .get_mut("mcp")
        .and_then(|value| value.as_object_mut())
    {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    if let Some(mcp) = config
        .get_mut("mcp")
        .and_then(|value| value.as_object_mut())
    {
        mcp.remove(id);
    }
    write_opencode_config(&config)
}

pub fn add_plugin(plugin_name: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    let plugins = config
        .get_mut("plugin")
        .and_then(|value| value.as_array_mut());

    match plugins {
        Some(items) => {
            if plugin_name.starts_with("oh-my-opencode")
                && !plugin_name.starts_with("oh-my-opencode-slim")
            {
                items.retain(|value| {
                    value
                        .as_str()
                        .map(|text| !text.starts_with("oh-my-opencode-slim"))
                        .unwrap_or(true)
                });
            } else if plugin_name.starts_with("oh-my-opencode-slim") {
                items.retain(|value| {
                    value
                        .as_str()
                        .map(|text| {
                            !text.starts_with("oh-my-opencode")
                                || text.starts_with("oh-my-opencode-slim")
                        })
                        .unwrap_or(true)
                });
            }

            if !items
                .iter()
                .any(|value| value.as_str() == Some(plugin_name))
            {
                items.push(Value::String(plugin_name.to_string()));
            }
        }
        None => {
            config["plugin"] = json!([plugin_name]);
        }
    }

    write_opencode_config(&config)
}

pub fn remove_plugin_by_prefix(prefix: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    if let Some(items) = config
        .get_mut("plugin")
        .and_then(|value| value.as_array_mut())
    {
        items.retain(|value| {
            value
                .as_str()
                .map(|text| {
                    if !text.starts_with(prefix) {
                        return true;
                    }
                    let rest = &text[prefix.len()..];
                    rest.starts_with('-')
                })
                .unwrap_or(true)
        });

        if items.is_empty() {
            config.as_object_mut().map(|obj| obj.remove("plugin"));
        }
    }

    write_opencode_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serial_test::serial;
    use tempfile::tempdir;

    #[test]
    #[serial]
    fn opencode_provider_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        let mut config = OpenCodeProviderConfig::default();
        config.options.base_url = Some("https://example.com/v1".to_string());
        config.options.api_key = Some("test-key".to_string());
        set_typed_provider("demo", &config)?;

        let providers = get_typed_providers()?;
        assert_eq!(
            providers
                .get("demo")
                .and_then(|item| item.options.base_url.as_deref()),
            Some("https://example.com/v1")
        );

        Ok(())
    }

    #[test]
    #[serial]
    fn opencode_mcp_round_trip() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());

        set_mcp_server(
            "server-a",
            json!({
                "type": "local",
                "command": ["npx", "-y", "@example/server"],
                "enabled": true
            }),
        )?;

        let servers = get_mcp_servers()?;
        assert_eq!(
            servers
                .get("server-a")
                .and_then(|value| value.get("type"))
                .and_then(Value::as_str),
            Some("local")
        );

        remove_mcp_server("server-a")?;
        assert!(!get_mcp_servers()?.contains_key("server-a"));

        Ok(())
    }
}
