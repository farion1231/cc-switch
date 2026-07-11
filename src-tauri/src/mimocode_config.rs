use crate::config::write_json_file;
use crate::error::AppError;
use crate::provider::MiMoCodeProviderConfig;
use crate::settings::get_mimocode_override_dir;
use indexmap::IndexMap;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub fn get_mimocode_dir() -> PathBuf {
    if let Some(override_dir) = get_mimocode_override_dir() {
        return override_dir;
    }

    crate::config::get_home_dir()
        .join(".config")
        .join("mimocode")
}

pub fn get_mimocode_config_path() -> PathBuf {
    get_mimocode_dir().join("mimocode.jsonc")
}

/// 获取 MimoCode SQLite 数据库路径
/// 优先级: MIMOCODE_DB 环境变量 > XDG_DATA_HOME > ~/.local/share/mimocode
pub fn get_mimocode_db_path() -> PathBuf {
    // 支持 MIMOCODE_DB 环境变量覆盖（忽略空字符串）
    if let Ok(custom_path) = std::env::var("MIMOCODE_DB") {
        if !custom_path.is_empty() {
            let path = PathBuf::from(&custom_path);
            if path.is_absolute() {
                return path;
            }
            // 相对路径基于数据目录
            return get_mimocode_data_dir().join(path);
        }
    }

    get_mimocode_data_dir().join("mimocode.db")
}

pub fn get_mimocode_data_dir() -> PathBuf {
    // 尊重 XDG_DATA_HOME（按 XDG 规范，空字符串视为未设置）
    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.is_empty() {
            return PathBuf::from(xdg_data).join("mimocode");
        }
    }

    // MimoCode 使用 xdg-basedir，不遵守 macOS/Windows 平台约定，
    // 所有平台默认都落在 ~/.local/share/mimocode
    crate::config::get_home_dir()
        .join(".local")
        .join("share")
        .join("mimocode")
}

pub fn read_mimocode_config() -> Result<Value, AppError> {
    let path = get_mimocode_config_path();

    if !path.exists() {
        return Ok(json!({
            "$schema": "https://mimo.xiaomi.com/mimocode/config.json"
        }));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse MimoCode config: {}: {e}",
            path.display()
        ))
    })
}

pub fn write_mimocode_config(config: &Value) -> Result<(), AppError> {
    let path = get_mimocode_config_path();
    write_json_file(&path, config)?;

    log::debug!("MimoCode config written to {path:?}");
    Ok(())
}

pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_mimocode_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_mimocode_config()?;

    if full_config.get("provider").is_none() {
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_mimocode_config(&full_config)
}

pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_mimocode_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    }

    write_mimocode_config(&config)
}

pub fn get_typed_providers() -> Result<IndexMap<String, MiMoCodeProviderConfig>, AppError> {
    let providers = get_providers()?;
    let mut result = IndexMap::new();

    for (id, value) in providers {
        match serde_json::from_value::<MiMoCodeProviderConfig>(value.clone()) {
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

pub fn set_typed_provider(id: &str, config: &MiMoCodeProviderConfig) -> Result<(), AppError> {
    let value = serde_json::to_value(config).map_err(|e| AppError::JsonSerialize { source: e })?;
    set_provider(id, value)
}

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_mimocode_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

pub fn set_mcp_server(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_mimocode_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), config);
    }

    write_mimocode_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_mimocode_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_mimocode_config(&config)
}
