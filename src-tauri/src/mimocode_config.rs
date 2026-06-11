use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_mimo_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const MIMOCODE_CONFIG_FILES_READ_ORDER: [&str; 3] =
    ["config.json", "mimocode.json", "mimocode.jsonc"];
const MIMOCODE_CONFIG_FILES_WRITE_ORDER: [&str; 3] =
    ["mimocode.jsonc", "mimocode.json", "config.json"];

pub fn get_mimo_dir() -> PathBuf {
    if let Some(override_dir) = get_mimo_override_dir() {
        return override_dir;
    }

    if let Ok(config_dir) = std::env::var("MIMOCODE_CONFIG_DIR") {
        if !config_dir.trim().is_empty() {
            return PathBuf::from(config_dir);
        }
    }

    if let Ok(mimo_home) = std::env::var("MIMOCODE_HOME") {
        if !mimo_home.trim().is_empty() {
            let profile_root = PathBuf::from(mimo_home);
            if profile_root.is_absolute() {
                return profile_root.join("config");
            }

            log::warn!(
                "Ignoring relative MIMOCODE_HOME={}; MiMo Code requires an absolute path",
                profile_root.display()
            );
        }
    }

    if let Ok(xdg_config_home) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg_config_home.trim().is_empty() {
            return PathBuf::from(xdg_config_home).join("mimocode");
        }
    }

    crate::config::get_home_dir()
        .join(".config")
        .join("mimocode")
}

fn env_config_path() -> Option<PathBuf> {
    if get_mimo_override_dir().is_some() {
        return None;
    }

    std::env::var("MIMOCODE_CONFIG")
        .ok()
        .map(|config_path| config_path.trim().to_string())
        .filter(|config_path| !config_path.is_empty())
        .map(PathBuf::from)
}

pub fn get_mimo_config_path() -> PathBuf {
    if let Some(config_path) = env_config_path() {
        return config_path;
    }

    let dir = get_mimo_dir();
    for file_name in MIMOCODE_CONFIG_FILES_WRITE_ORDER {
        let candidate = dir.join(file_name);
        if candidate.exists() {
            return candidate;
        }
    }
    dir.join("mimocode.json")
}

fn read_mimo_config_file(path: &PathBuf) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse MiMo Code config: {}: {e}",
            path.display()
        ))
    })
}

fn merge_json(target: &mut Value, source: &Value) {
    match (target, source) {
        (Value::Object(target_obj), Value::Object(source_obj)) => {
            for (key, source_value) in source_obj {
                match target_obj.get_mut(key) {
                    Some(target_value) => merge_json(target_value, source_value),
                    None => {
                        target_obj.insert(key.clone(), source_value.clone());
                    }
                }
            }
        }
        (target_value, source_value) => {
            *target_value = source_value.clone();
        }
    }
}

pub fn read_mimo_config() -> Result<Value, AppError> {
    if let Some(config_path) = env_config_path() {
        return read_mimo_config_file(&config_path);
    }

    let dir = get_mimo_dir();
    let mut merged = json!({});

    for file_name in MIMOCODE_CONFIG_FILES_READ_ORDER {
        let path = dir.join(file_name);
        if path.exists() {
            let source = read_mimo_config_file(&path)?;
            merge_json(&mut merged, &source);
        }
    }

    Ok(merged)
}

pub fn write_mimo_config(config: &Value) -> Result<(), AppError> {
    let path = get_mimo_config_path();
    write_json_file(&path, config)?;

    log::debug!("MiMo Code config written to {path:?}");
    Ok(())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_mimo_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_mimo_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_mimo_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_mimo_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_mimo_config(&config)
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
                log::warn!("Failed to parse MiMo Code provider '{id}': {e}");
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
    let config = read_mimo_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_mimo_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_mimo_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_mimo_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_mimo_config(&config)
}
