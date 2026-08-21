use crate::app_config::{McpApps, McpServer};
use crate::config::serialize_json_file_contents;
use crate::error::AppError;
use serde_json::{Map, Value};
use std::fs;
use std::path::{Path, PathBuf};

const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

pub fn config_path() -> Result<PathBuf, AppError> {
    Ok(crate::copilot_byok::copilot_cli_home()?.join("mcp-config.json"))
}

fn read_root(path: &Path) -> Result<Map<String, Value>, AppError> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "Copilot CLI MCP target is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Copilot CLI MCP config exceeds {} MiB: {}",
            MAX_CONFIG_BYTES / 1024 / 1024,
            path.display()
        )));
    }

    let text = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    serde_json::from_str::<Value>(&text)
        .map_err(|error| AppError::json(path, error))?
        .as_object()
        .cloned()
        .ok_or_else(|| {
            AppError::Config(format!(
                "Copilot CLI MCP config must be a JSON object: {}",
                path.display()
            ))
        })
}

fn updated_contents(path: &Path, id: &str, server: Option<&Value>) -> Result<Vec<u8>, AppError> {
    let mut root = read_root(path)?;
    let servers = root
        .entry("mcpServers".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::Config(format!(
                "Copilot CLI MCP 'mcpServers' field must be an object: {}",
                path.display()
            ))
        })?;

    match server {
        Some(server) => {
            servers.insert(id.to_string(), server.clone());
        }
        None => {
            servers.remove(id);
        }
    }
    serialize_json_file_contents(&Value::Object(root))
}

fn update(id: &str, server: Option<&Value>) -> Result<(), AppError> {
    if server.is_some_and(|server| !server.is_object()) {
        return Err(AppError::InvalidInput(
            "Copilot CLI MCP server configuration must be a JSON object".to_string(),
        ));
    }
    let path = config_path()?;
    if server.is_none() && !path.exists() {
        return Ok(());
    }
    let contents = updated_contents(&path, id, server)?;
    if contents.len() as u64 > MAX_CONFIG_BYTES {
        return Err(AppError::InvalidInput(format!(
            "Copilot CLI MCP config exceeds {} MiB",
            MAX_CONFIG_BYTES / 1024 / 1024
        )));
    }
    // MCP server definitions may contain credentials in headers or env. Keep
    // the official file private on Unix while retaining atomic replacement.
    crate::config::atomic_write_private(&path, &contents)
}

pub fn sync_single_server_to_copilot_cli(id: &str, server: &Value) -> Result<(), AppError> {
    update(id, Some(server))
}

pub fn remove_server_from_copilot_cli(id: &str) -> Result<(), AppError> {
    update(id, None)
}

pub fn import_from_copilot_cli() -> Result<Vec<McpServer>, AppError> {
    let path = config_path()?;
    let root = read_root(&path)?;
    let Some(servers) = root.get("mcpServers").and_then(Value::as_object) else {
        return Ok(Vec::new());
    };

    Ok(servers
        .iter()
        .map(|(id, server)| McpServer {
            id: id.clone(),
            name: id.clone(),
            server: server.clone(),
            apps: McpApps {
                copilot_cli: true,
                ..McpApps::default()
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn updates_official_mcp_servers_field_without_dropping_other_keys() {
        let temp = tempfile::tempdir().expect("temp directory");
        let path = temp.path().join("mcp-config.json");
        fs::write(
            &path,
            r#"{"other":true,"mcpServers":{"old":{"command":"old"}}}"#,
        )
        .expect("seed config");

        let contents =
            updated_contents(&path, "new", Some(&json!({"command":"new"}))).expect("update config");
        let value: Value = serde_json::from_slice(&contents).expect("parse config");

        assert_eq!(value["other"], true);
        assert_eq!(value["mcpServers"]["old"]["command"], "old");
        assert_eq!(value["mcpServers"]["new"]["command"], "new");
    }
}
