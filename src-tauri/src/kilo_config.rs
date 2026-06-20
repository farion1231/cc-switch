use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::OpenCodeProviderConfig;
use crate::settings::get_kilo_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

const KILO_SCHEMA: &str = "https://app.kilo.ai/config.json";

pub fn get_kilo_dir() -> PathBuf {
    if let Some(override_dir) = get_kilo_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir()
        .join(".config")
        .join("kilo")
}

pub fn get_kilo_config_path() -> PathBuf {
    get_kilo_dir().join("kilo.jsonc")
}

/// 获取 Kilo SQLite 数据库路径
pub fn get_kilo_db_path() -> PathBuf {
    if let Ok(custom_path) = std::env::var("KILO_DB") {
        if !custom_path.is_empty() {
            let path = PathBuf::from(&custom_path);
            if path.is_absolute() {
                return path;
            }
            return get_kilo_data_dir().join(path);
        }
    }

    get_kilo_data_dir().join("kilo.db")
}

/// Return the Kilo base data directory (`$XDG_DATA_HOME/kilo` or `~/.local/share/kilo`).
fn get_kilo_base_dir() -> PathBuf {
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.is_empty() {
            return PathBuf::from(xdg_data).join("kilo");
        }
    }

    crate::config::get_home_dir()
        .join(".local")
        .join("share")
        .join("kilo")
}

/// Return the Kilo JSON storage directory (legacy flat-file layout).
pub fn get_kilo_data_dir() -> PathBuf {
    get_kilo_base_dir().join("storage")
}

pub fn read_kilo_config() -> Result<Value, AppError> {
    let path = get_kilo_config_path();

    if !path.exists() {
        return Ok(json!({
            "$schema": KILO_SCHEMA
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse Kilo config: {}: {e}",
            path.display()
        ))
    })
}

pub fn write_kilo_config(config: &Value) -> Result<(), AppError> {
    let path = get_kilo_config_path();
    write_json_file(&path, config)?;

    log::debug!("Kilo config written to {path:?}");
    Ok(())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_kilo_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_kilo_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_kilo_config(&full_config)
}

#[allow(dead_code)]
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_kilo_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_kilo_config(&config)
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
                log::warn!("Failed to parse Kilo provider '{id}': {e}");
            }
        }
    }

    Ok(result)
}

pub fn set_typed_provider(id: &str, config: &OpenCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

#[allow(dead_code)]
pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_kilo_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_kilo_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_kilo_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_kilo_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_kilo_config(&config)
}
