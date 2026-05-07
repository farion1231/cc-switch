use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::atomic_write;
use crate::error::AppError;
use crate::qwen_config::get_qwen_settings_path;

fn user_config_path() -> PathBuf {
    get_qwen_settings_path()
}

fn read_json_value(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    let value: Value = serde_json::from_str(&content).map_err(|e| AppError::json(path, e))?;
    Ok(value)
}

fn write_json_value(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let json =
        serde_json::to_string_pretty(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    atomic_write(path, json.as_bytes())
}

/// 读取 Qwen settings.json 中的 mcpServers 映射。
///
/// 反向格式转换到统一 MCP 结构：
/// - `httpUrl` -> `url` + `type: "http"`
/// - 仅有 `url` -> `type: "sse"`
/// - 仅有 `command` -> `type: "stdio"`
pub fn read_mcp_servers_map() -> Result<std::collections::HashMap<String, Value>, AppError> {
    let path = user_config_path();
    if !path.exists() {
        return Ok(std::collections::HashMap::new());
    }

    let root = read_json_value(&path)?;
    let mut servers: std::collections::HashMap<String, Value> = root
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default();

    for spec in servers.values_mut() {
        if let Some(obj) = spec.as_object_mut() {
            if let Some(http_url) = obj.remove("httpUrl") {
                obj.insert("url".to_string(), http_url);
                obj.insert("type".to_string(), Value::String("http".to_string()));
            } else if obj.get("type").is_none() {
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

/// 将统一 MCP 结构写回 Qwen settings.json 的 `mcpServers`。
///
/// 只更新 `mcpServers` 字段，保留其他根级配置。
pub fn set_mcp_servers_map(
    servers: &std::collections::HashMap<String, Value>,
) -> Result<(), AppError> {
    let path = user_config_path();
    let mut root = if path.exists() {
        read_json_value(&path)?
    } else {
        serde_json::json!({})
    };

    let mut out: Map<String, Value> = Map::new();
    for (id, spec) in servers {
        let mut obj = if let Some(map) = spec.as_object() {
            map.clone()
        } else {
            return Err(AppError::McpValidation(format!(
                "MCP 服务器 '{id}' 不是对象"
            )));
        };

        if let Some(server_val) = obj.remove("server") {
            let server_obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
            obj = server_obj;
        }

        let transport_type = obj.get("type").and_then(|v| v.as_str());
        if transport_type == Some("http") {
            if let Some(url_value) = obj.remove("url") {
                obj.insert("httpUrl".to_string(), url_value);
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

    let obj = root
        .as_object_mut()
        .ok_or_else(|| AppError::Config("~/.qwen/settings.json 根必须是对象".into()))?;
    obj.insert("mcpServers".into(), Value::Object(out));

    write_json_value(&path, &root)?;
    Ok(())
}
