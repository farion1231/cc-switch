use crate::config::{get_home_dir, read_json_file, write_json_file};
use crate::error::AppError;
use serde_json::Value;
use std::fs;
use std::path::PathBuf;

/// 获取 Antigravity 2.0 配置目录路径
pub fn get_antigravity_dir() -> PathBuf {
    get_home_dir().join(".gemini").join("config")
}

/// 获取 Antigravity 2.0 config.json 路径
pub fn get_antigravity_config_path() -> PathBuf {
    get_antigravity_dir().join("config.json")
}

/// 写入 Antigravity live 配置
pub fn write_antigravity_live_config(config_value: &Value) -> Result<(), AppError> {
    let config_path = get_antigravity_config_path();

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let mut merged = if config_path.exists() {
        read_json_file::<Value>(&config_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let (Some(merged_obj), Some(config_obj)) = (merged.as_object_mut(), config_value.as_object()) {
        for (k, v) in config_obj {
            merged_obj.insert(k.clone(), v.clone());
        }
    }

    write_json_file(&config_path, &merged)?;

    Ok(())
}

/// Write Google OAuth settings to Antigravity config
pub fn write_google_oauth_settings() -> Result<(), AppError> {
    let config_path = get_antigravity_config_path();

    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let mut config: Value = if config_path.exists() {
        read_json_file(&config_path).unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        if !obj.contains_key("security") {
            obj.insert("security".to_string(), serde_json::json!({}));
        }
        if let Some(security) = obj.get_mut("security").and_then(|v| v.as_object_mut()) {
            if !security.contains_key("auth") {
                security.insert("auth".to_string(), serde_json::json!({}));
            }
            if let Some(auth) = security.get_mut("auth").and_then(|v| v.as_object_mut()) {
                auth.insert(
                    "selectedType".to_string(),
                    Value::String("oauth-personal".to_string()),
                );
            }
        }
    }

    write_json_file(&config_path, &config)?;

    Ok(())
}
