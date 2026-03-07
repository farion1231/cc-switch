//! Gemini MCP configuration handling.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::config::atomic_write;
use crate::error::AppError;
use crate::gemini_config::{get_gemini_dir, get_gemini_settings_path};

fn should_sync_gemini_mcp() -> bool {
    get_gemini_dir().exists()
}

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }

    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    serde_json::from_str(&content).map_err(|e| AppError::json(path, e))
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }

    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let path = get_gemini_settings_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let root = read_json_value(&path)?;
    let mut servers: HashMap<String, Value> = root
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    for spec in servers.values_mut() {
        if let Some(obj) = spec.as_object_mut() {
            if let Some(http_url) = obj.remove("httpUrl") {
                obj.insert("url".to_string(), http_url);
                obj.insert("type".to_string(), Value::String("http".to_string()));
            }

            if obj.get("type").is_none() {
                if obj.contains_key("command") {
                    obj.insert("type".to_string(), Value::String("stdio".to_string()));
                } else if obj.contains_key("url") {
                    obj.insert("type".to_string(), Value::String("sse".to_string()));
                }
            }
        }
    }

    Ok(servers)
}

pub fn set_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    let path = get_gemini_settings_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    let mut out = Map::new();
    for (id, spec) in servers {
        let mut obj = spec.as_object().cloned().ok_or_else(|| {
            AppError::McpValidation(format!("MCP 服务器 '{id}' 不是 JSON 对象"))
        })?;

        if let Some(server_val) = obj.remove("server") {
            obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
        }

        if obj.get("type").and_then(Value::as_str) == Some("http") {
            if let Some(url) = obj.remove("url") {
                obj.insert("httpUrl".to_string(), url);
            }
        }

        obj.remove("type");
        obj.remove("enabled");
        obj.remove("source");
        obj.remove("id");
        obj.remove("name");
        obj.remove("description");
        obj.remove("tags");
        obj.remove("homepage");
        obj.remove("docs");

        out.insert(id.clone(), Value::Object(obj));
    }

    let root_obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("Gemini MCP 根配置必须是对象".into()))?;
    root_obj.insert("mcpServers".into(), Value::Object(out));

    write_json_value(&path, &root)
}

pub fn sync_single_server_to_gemini(id: &str, spec: &Value) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }

    let mut current = read_mcp_servers_map()?;
    current.insert(id.to_string(), spec.clone());
    set_mcp_servers_map(&current)
}

pub fn remove_server_from_gemini(id: &str) -> Result<(), AppError> {
    if !should_sync_gemini_mcp() {
        return Ok(());
    }

    let mut current = read_mcp_servers_map()?;
    current.remove(id);
    set_mcp_servers_map(&current)
}
