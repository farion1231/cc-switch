//! Claude MCP configuration handling.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde_json::{Map, Value};

use crate::config::{atomic_write, get_claude_config_dir, get_claude_mcp_path};
use crate::error::AppError;

fn should_sync_claude_mcp() -> bool {
    get_claude_config_dir().exists() || get_claude_mcp_path().exists()
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
    let path = get_claude_mcp_path();
    if !path.exists() {
        return Ok(HashMap::new());
    }

    let root = read_json_value(&path)?;
    Ok(root
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default())
}

pub fn set_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    let path = get_claude_mcp_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    let mut out = Map::new();
    for (id, spec) in servers {
        let mut obj = spec
            .as_object()
            .cloned()
            .ok_or_else(|| AppError::McpValidation(format!("MCP 服务器 '{id}' 不是 JSON 对象")))?;

        if let Some(server_val) = obj.remove("server") {
            obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
        }

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
        .ok_or_else(|| AppError::Config("Claude MCP 根配置必须是对象".into()))?;
    root_obj.insert("mcpServers".into(), Value::Object(out));

    write_json_value(&path, &root)
}

pub fn sync_single_server_to_claude(id: &str, spec: &Value) -> Result<(), AppError> {
    if !should_sync_claude_mcp() {
        return Ok(());
    }

    let mut current = read_mcp_servers_map()?;
    current.insert(id.to_string(), spec.clone());
    set_mcp_servers_map(&current)
}

pub fn remove_server_from_claude(id: &str) -> Result<(), AppError> {
    if !should_sync_claude_mcp() {
        return Ok(());
    }

    let mut current = read_mcp_servers_map()?;
    current.remove(id);
    set_mcp_servers_map(&current)
}
