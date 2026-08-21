use crate::config::{get_home_dir, write_json_file};
use crate::error::AppError;
use serde_json::{json, Map, Value};
use std::path::PathBuf;

pub fn get_codefree_db_path() -> PathBuf {
    if let Ok(custom_path) = std::env::var("CODEFREE_DB") {
        if !custom_path.is_empty() {
            let path = PathBuf::from(&custom_path);
            if path.is_absolute() {
                return path;
            }
            return get_codefree_data_dir().join(path);
        }
    }

    get_codefree_data_dir().join("codefree.db")
}

pub fn get_codefree_dir() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_codefree_override_dir() {
        return override_dir;
    }

    get_home_dir().join(".codefree-o")
}

pub fn get_codefree_data_dir() -> PathBuf {
    if let Some(override_dir) = crate::settings::get_codefree_override_dir() {
        return override_dir.join(".local").join("share");
    }

    if let Ok(xdg_data) = std::env::var("XDG_DATA_HOME") {
        if !xdg_data.is_empty() {
            return PathBuf::from(xdg_data).join("codefree-o");
        }
    }

    get_home_dir()
        .join(".codefree-o")
        .join(".local")
        .join("share")
}

pub fn get_codefree_config_dir() -> PathBuf {
    get_home_dir().join(".codefree-o").join(".config")
}

pub fn get_codefree_config_path() -> PathBuf {
    get_codefree_config_dir().join("codefree.json")
}

pub fn read_codefree_config() -> Result<Value, AppError> {
    let path = get_codefree_config_path();

    if !path.exists() {
        return Ok(json!({}));
    }

    let content = std::fs::read_to_string(&path).map_err(|e| AppError::io(&path, e))?;
    json5::from_str(&content).map_err(|e| {
        AppError::Config(format!(
            "Failed to parse CodeFree config: {}: {e}",
            path.display()
        ))
    })
}

pub fn write_codefree_config(config: &Value) -> Result<(), AppError> {
    let path = get_codefree_config_path();
    write_json_file(&path, config)?;

    log::debug!("CodeFree config written to {path:?}");
    Ok(())
}

#[allow(dead_code)]
pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_codefree_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 读取 CodeFree 配置中的所有供应商
#[allow(dead_code)]
pub fn get_providers() -> Result<Map<String, Value>, AppError> {
    let config = read_codefree_config()?;
    Ok(config
        .get("provider")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
}

/// 写入/更新 CodeFree 配置中的单个供应商
///
/// CodeFree 采用 Additive 模式，供应商共存于 `codefree.json` 的 `provider` 段。
pub fn set_provider(id: &str, config: Value) -> Result<(), AppError> {
    let mut full_config = read_codefree_config()?;

    if !full_config.get("provider").is_some_and(Value::is_object) {
        if full_config.get("provider").is_some() {
            log::warn!("codefree.json 的 provider 不是对象，已重置为空对象");
        }
        full_config["provider"] = json!({});
    }

    if let Some(providers) = full_config
        .get_mut("provider")
        .and_then(|v| v.as_object_mut())
    {
        providers.insert(id.to_string(), config);
    }

    write_codefree_config(&full_config)
}

/// 从 CodeFree 配置中移除指定供应商
#[allow(dead_code)]
pub fn remove_provider(id: &str) -> Result<(), AppError> {
    let mut config = read_codefree_config()?;

    if let Some(providers) = config.get_mut("provider").and_then(|v| v.as_object_mut()) {
        providers.remove(id);
    } else if config.get("provider").is_some() {
        log::warn!("codefree.json 的 provider 不是对象，无法删除供应商 '{id}'");
    }

    write_codefree_config(&config)
}

pub fn set_mcp_server(id: &str, spec: Value) -> Result<(), AppError> {
    let mut full_config = read_codefree_config()?;

    if full_config.get("mcp").is_none() {
        full_config["mcp"] = json!({});
    }

    if let Some(mcp) = full_config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.insert(id.to_string(), spec);
    }

    write_codefree_config(&full_config)
}

pub fn remove_mcp_server(id: &str) -> Result<(), AppError> {
    let mut config = read_codefree_config()?;

    if let Some(mcp) = config.get_mut("mcp").and_then(|v| v.as_object_mut()) {
        mcp.remove(id);
    }

    write_codefree_config(&config)
}
