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

pub fn get_mcp_servers() -> Result<Map<String, Value>, AppError> {
    let config = read_codefree_config()?;
    Ok(config
        .get("mcp")
        .and_then(|v| v.as_object())
        .cloned()
        .unwrap_or_default())
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
