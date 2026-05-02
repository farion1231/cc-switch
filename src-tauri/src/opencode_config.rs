use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_opencode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const STANDARD_OMO_PLUGIN_PREFIXES: [&str; 2] = ["oh-my-openagent", "oh-my-opencode"];
const SLIM_OMO_PLUGIN_PREFIXES: [&str; 1] = ["oh-my-opencode-slim"];

fn matches_plugin_prefix(plugin_name: &str, prefix: &str) -> bool {
    plugin_name == prefix
        || plugin_name
            .strip_prefix(prefix)
            .map(|suffix| suffix.starts_with('@'))
            .unwrap_or(false)
}

fn matches_any_plugin_prefix(plugin_name: &str, prefixes: &[&str]) -> bool {
    prefixes
        .iter()
        .any(|prefix| matches_plugin_prefix(plugin_name, prefix))
}

fn canonicalize_plugin_name(plugin_name: &str) -> String {
    if let Some(suffix) = plugin_name.strip_prefix("oh-my-opencode") {
        if suffix.is_empty() || suffix.starts_with('@') {
            return format!("oh-my-openagent{suffix}");
        }
    }
    plugin_name.to_string()
}

pub fn get_opencode_dir() -> PathBuf {
    if let Some(override_dir) = get_opencode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir()
        .join(".config")
        .join("opencode")
}

pub fn get_opencode_config_path() -> PathBuf {
    let dir = get_opencode_dir();

    // Prefer opencode.jsonc if it exists, fallback to opencode.json
    let jsonc_path = dir.join("opencode.jsonc");
    if jsonc_path.exists() {
        return jsonc_path;
    }

    dir.join("opencode.json")
}

#[allow(dead_code)]
pub fn get_opencode_env_path() -> PathBuf {
    get_opencode_dir().join(".env")
}

pub fn read_opencode_config() -> Result<Value, AppError> {
    let path = get_opencode_config_path();

    if !path.exists() {
        return Ok(json!({
            "$schema": "https://opencode.ai/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse OpenCode config: {}: {e}",
            path.display()
        ))
    })
}

pub fn write_opencode_config(config: &Value) -> Result<(), AppError> {
    let path = get_opencode_config_path();
    write_json_file(&path, config)?;

    log::debug!("OpenCode config written to {path:?}");
    Ok(())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_opencode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
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
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
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
            Err(e) => {
                log::warn!("Failed to parse provider '{id}': {e}");
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
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_opencode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_opencode_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_opencode_config(&config)
}

pub fn add_plugin(plugin_name: &str) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;
    let normalized_plugin_name = canonicalize_plugin_name(plugin_name);

    let plugins = config.get_mut("plugin").and_then(|v| v.as_array_mut());

    match plugins {
        Some(arr) => {
            // Mutual exclusion: standard OMO and OMO Slim cannot coexist as plugins
            if matches_any_plugin_prefix(&normalized_plugin_name, &STANDARD_OMO_PLUGIN_PREFIXES) {
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| {
                            !matches_any_plugin_prefix(s, &STANDARD_OMO_PLUGIN_PREFIXES)
                                && !matches_any_plugin_prefix(s, &SLIM_OMO_PLUGIN_PREFIXES)
                        })
                        .unwrap_or(true)
                });
            } else if matches_any_plugin_prefix(&normalized_plugin_name, &SLIM_OMO_PLUGIN_PREFIXES)
            {
                arr.retain(|v| {
                    v.as_str()
                        .map(|s| {
                            !matches_any_plugin_prefix(s, &STANDARD_OMO_PLUGIN_PREFIXES)
                                && !matches_any_plugin_prefix(s, &SLIM_OMO_PLUGIN_PREFIXES)
                        })
                        .unwrap_or(true)
                });
            }

            let already_exists = arr
                .iter()
                .any(|v| v.as_str() == Some(normalized_plugin_name.as_str()));
            if !already_exists {
                arr.push(Value::String(normalized_plugin_name));
            }
        }
        None => {
            config["plugin"] = json!([normalized_plugin_name]);
        }
    }

    write_opencode_config(&config)
}

pub fn remove_plugins_by_prefixes(prefixes: &[&str]) -> Result<(), AppError> {
    let mut config = read_opencode_config()?;

    if let Some(arr) = config.get_mut("plugin").and_then(|v| v.as_array_mut()) {
        arr.retain(|v| {
            v.as_str()
                .map(|s| !matches_any_plugin_prefix(s, prefixes))
                .unwrap_or(true)
        });

        if arr.is_empty() {
            config.as_object_mut().map(|obj| obj.remove("plugin"));
        }
    }

    write_opencode_config(&config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn setup_test_env() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let opencode_dir = temp_dir.path().join(".config").join("opencode");
        fs::create_dir_all(&opencode_dir).unwrap();
        (temp_dir, opencode_dir)
    }

    #[test]
    fn test_read_opencode_config_prefers_jsonc() {
        let (_temp, opencode_dir) = setup_test_env();

        // Create both .json and .jsonc files
        let json_path = opencode_dir.join("opencode.json");
        let jsonc_path = opencode_dir.join("opencode.jsonc");

        fs::write(&json_path, r#"{"provider": {"test-json": {}}}"#).unwrap();
        fs::write(&jsonc_path, r#"{"provider": {"test-jsonc": {}}}"#).unwrap();

        // Set override dir to use temp directory
        std::env::set_var("HOME", opencode_dir.parent().unwrap().parent().unwrap());

        // Should prefer .jsonc file
        let path = get_opencode_config_path();
        assert!(path.ends_with("opencode.jsonc"));

        // Clean up env var
        std::env::remove_var("HOME");
    }

    #[test]
    fn test_read_opencode_config_fallback_to_json() {
        let (_temp, opencode_dir) = setup_test_env();

        // Create only .json file
        let json_path = opencode_dir.join("opencode.json");
        fs::write(&json_path, r#"{"provider": {"test": {}}}"#).unwrap();

        std::env::set_var("HOME", opencode_dir.parent().unwrap().parent().unwrap());

        // Should fallback to .json file
        let path = get_opencode_config_path();
        assert!(path.ends_with("opencode.json"));

        std::env::remove_var("HOME");
    }

    #[test]
    fn test_read_opencode_config_with_comments() {
        let (_temp, opencode_dir) = setup_test_env();

        // Create .jsonc file with comments
        let jsonc_path = opencode_dir.join("opencode.jsonc");
        let config_with_comments = r#"{
            // This is a comment
            "$schema": "https://opencode.ai/config.json",
            /* Multi-line
               comment */
            "provider": {
                "test-provider": {
                    "apiKey": "test-key" // Inline comment
                }
            }
        }"#;
        fs::write(&jsonc_path, config_with_comments).unwrap();

        std::env::set_var("HOME", opencode_dir.parent().unwrap().parent().unwrap());

        // Should successfully parse config with comments
        let config = read_opencode_config().unwrap();
        assert!(config.get("provider").is_some());
        assert!(config["provider"]
            .get("test-provider")
            .and_then(|p| p.get("apiKey"))
            .and_then(|k| k.as_str())
            == Some("test-key"));

        std::env::remove_var("HOME");
    }

    #[test]
    fn test_read_opencode_config_trailing_commas() {
        let (_temp, opencode_dir) = setup_test_env();

        // Create .jsonc file with trailing commas
        let jsonc_path = opencode_dir.join("opencode.jsonc");
        let config_with_trailing = r#"{
            "$schema": "https://opencode.ai/config.json",
            "provider": {
                "test-provider": {
                    "apiKey": "test-key",
                    "baseUrl": "https://api.example.com",
                },
            },
        }"#;
        fs::write(&jsonc_path, config_with_trailing).unwrap();

        std::env::set_var("HOME", opencode_dir.parent().unwrap().parent().unwrap());

        // Should successfully parse config with trailing commas
        let config = read_opencode_config().unwrap();
        assert!(config.get("provider").is_some());

        std::env::remove_var("HOME");
    }
}
