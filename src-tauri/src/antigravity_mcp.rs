use serde_json::{Map, Value};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::atomic_write;
use crate::error::AppError;

fn config_candidates() -> Vec<PathBuf> {
    config_candidates_for_root(&crate::antigravity_config::get_antigravity_dir())
}

fn config_candidates_for_root(root: &Path) -> Vec<PathBuf> {
    vec![
        root.join("config").join("mcp_config.json"),
        root.join("antigravity-ide").join("mcp_config.json"),
        root.join("antigravity").join("mcp_config.json"),
    ]
}

fn writable_config_path() -> PathBuf {
    config_candidates()
        .into_iter()
        .find(|path| path.exists())
        .unwrap_or_else(|| {
            crate::antigravity_config::get_antigravity_dir()
                .join("config")
                .join("mcp_config.json")
        })
}

fn read_json(path: &Path) -> Result<Value, AppError> {
    if !path.exists() {
        return Ok(serde_json::json!({}));
    }
    let content = fs::read_to_string(path).map_err(|e| AppError::io(path, e))?;
    if content.trim().is_empty() {
        return Ok(serde_json::json!({}));
    }
    serde_json::from_str(&content).map_err(|e| AppError::json(path, e))
}

fn write_json(path: &Path, value: &Value) -> Result<(), AppError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| AppError::io(parent, e))?;
    }
    let content =
        serde_json::to_string_pretty(value).map_err(|source| AppError::JsonSerialize { source })?;
    atomic_write(path, content.as_bytes())
}

pub fn read_mcp_servers_map() -> Result<HashMap<String, Value>, AppError> {
    let path = config_candidates()
        .into_iter()
        .find(|candidate| candidate.exists());
    let Some(path) = path else {
        return Ok(HashMap::new());
    };

    let root = read_json(&path)?;
    let mut servers: HashMap<String, Value> = root
        .get("mcpServers")
        .and_then(Value::as_object)
        .map(|servers| {
            servers
                .iter()
                .map(|(id, spec)| (id.clone(), spec.clone()))
                .collect()
        })
        .unwrap_or_default();

    for spec in servers.values_mut() {
        normalize_server_for_read(spec);
    }

    Ok(servers)
}

pub fn set_mcp_servers_map(servers: &HashMap<String, Value>) -> Result<(), AppError> {
    let path = writable_config_path();
    let mut root = read_json(&path)?;
    if !root.is_object() {
        root = serde_json::json!({});
    }

    let mut output = Map::new();
    for (id, spec) in servers {
        output.insert(id.clone(), normalize_server_for_write(id, spec)?);
    }

    root.as_object_mut()
        .expect("root normalized to object")
        .insert("mcpServers".to_string(), Value::Object(output));
    write_json(&path, &root)
}

fn normalize_server_for_read(spec: &mut Value) {
    let Some(object) = spec.as_object_mut() else {
        return;
    };
    if let Some(server_url) = object.remove("serverUrl") {
        object.insert("url".to_string(), server_url);
        object.insert("type".to_string(), Value::String("http".to_string()));
    } else if object.contains_key("command") && !object.contains_key("type") {
        object.insert("type".to_string(), Value::String("stdio".to_string()));
    }
}

fn normalize_server_for_write(id: &str, spec: &Value) -> Result<Value, AppError> {
    let mut object = spec.as_object().cloned().ok_or_else(|| {
        AppError::McpValidation(format!(
            "Antigravity 2.0 MCP server '{id}' must be an object"
        ))
    })?;

    if let Some(server) = object.remove("server") {
        object = server.as_object().cloned().ok_or_else(|| {
            AppError::McpValidation(format!(
                "Antigravity 2.0 MCP server '{id}' has an invalid server field"
            ))
        })?;
    }

    let transport = object
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if transport == "http" || transport == "sse" || object.contains_key("url") {
        if let Some(url) = object.remove("url").or_else(|| object.remove("httpUrl")) {
            object.insert("serverUrl".to_string(), url);
        }
    }

    for key in [
        "type",
        "enabled",
        "source",
        "id",
        "name",
        "description",
        "tags",
        "homepage",
        "docs",
        "startup_timeout_sec",
        "startup_timeout_ms",
        "tool_timeout_sec",
        "tool_timeout_ms",
    ] {
        object.remove(key);
    }

    Ok(Value::Object(object))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn prefers_current_antigravity_mcp_config_path() {
        let temp = tempdir().expect("tempdir");
        let candidates = config_candidates_for_root(temp.path());

        assert_eq!(
            candidates[0],
            temp.path().join("config").join("mcp_config.json")
        );
    }

    #[test]
    fn normalizes_antigravity_remote_server_for_import() {
        let mut spec = serde_json::json!({
            "serverUrl": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer token" }
        });

        normalize_server_for_read(&mut spec);

        assert_eq!(spec["type"], "http");
        assert_eq!(spec["url"], "https://example.com/mcp");
        assert!(spec.get("serverUrl").is_none());
    }

    #[test]
    fn normalizes_unified_remote_server_for_antigravity() {
        let spec = serde_json::json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "enabled": true,
            "headers": { "Authorization": "Bearer token" }
        });

        let output = normalize_server_for_write("remote", &spec).expect("normalize server");

        assert_eq!(output["serverUrl"], "https://example.com/mcp");
        assert_eq!(output["headers"]["Authorization"], "Bearer token");
        assert!(output.get("type").is_none());
        assert!(output.get("enabled").is_none());
    }

    #[test]
    fn unwraps_unified_stdio_server_for_antigravity() {
        let spec = serde_json::json!({
            "server": {
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "example"]
            },
            "enabled": true
        });

        let output = normalize_server_for_write("stdio", &spec).expect("normalize server");

        assert_eq!(output["command"], "npx");
        assert_eq!(output["args"], serde_json::json!(["-y", "example"]));
        assert!(output.get("type").is_none());
    }
}
