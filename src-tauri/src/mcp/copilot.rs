use crate::app_config::{McpApps, McpServer};
use crate::config::serialize_json_file_contents;
use crate::error::AppError;
use crate::file_transaction::{commit_file_updates, FileUpdate};
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const MAX_CONFIG_BYTES: u64 = 8 * 1024 * 1024;

fn target_paths() -> Result<Vec<PathBuf>, AppError> {
    crate::copilot_byok::selected_mcp_paths()
}

fn read_root(path: &Path) -> Result<Map<String, Value>, AppError> {
    if !path.exists() {
        return Ok(Map::new());
    }
    let metadata = fs::symlink_metadata(path).map_err(|error| AppError::io(path, error))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(AppError::InvalidInput(format!(
            "VS Code MCP target is not a regular file: {}",
            path.display()
        )));
    }
    if metadata.len() > MAX_CONFIG_BYTES {
        return Err(AppError::InvalidInput(format!(
            "VS Code MCP config exceeds {} MiB: {}",
            MAX_CONFIG_BYTES / 1024 / 1024,
            path.display()
        )));
    }

    let text = fs::read_to_string(path).map_err(|error| AppError::io(path, error))?;
    if text.trim().is_empty() {
        return Ok(Map::new());
    }
    json5::from_str::<Value>(&text)
        .map_err(|error| {
            AppError::Config(format!(
                "Failed to parse VS Code MCP config {}: {error}",
                path.display()
            ))
        })?
        .as_object()
        .cloned()
        .ok_or_else(|| {
            AppError::Config(format!(
                "VS Code MCP config must be a JSON object: {}",
                path.display()
            ))
        })
}

fn updated_server_contents(
    path: &Path,
    id: &str,
    server: Option<&Value>,
) -> Result<Vec<u8>, AppError> {
    let mut root = read_root(path)?;
    let servers = root
        .entry("servers".to_string())
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or_else(|| {
            AppError::Config(format!(
                "VS Code MCP 'servers' field must be an object: {}",
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

fn update_paths(paths: Vec<PathBuf>, id: &str, server: Option<&Value>) -> Result<(), AppError> {
    if server.is_some_and(|server| !server.is_object()) {
        return Err(AppError::InvalidInput(
            "VS Code MCP server configuration must be a JSON object".to_string(),
        ));
    }

    // Read and validate every selected Profile before the first write. The
    // shared transaction then restores earlier targets if a later write fails.
    let updates = paths
        .into_iter()
        .map(|path| {
            updated_server_contents(&path, id, server)
                .map(|contents| FileUpdate::write(path, contents))
        })
        .collect::<Result<Vec<_>, _>>()?;
    commit_file_updates(updates, Some(MAX_CONFIG_BYTES), "VS Code MCP config")
}

pub fn sync_single_server_to_copilot(id: &str, server: &Value) -> Result<(), AppError> {
    update_paths(target_paths()?, id, Some(server))
}

pub fn remove_server_from_copilot(id: &str) -> Result<(), AppError> {
    update_paths(
        target_paths()?
            .into_iter()
            .filter(|path| path.exists())
            .collect(),
        id,
        None,
    )
}

pub fn import_from_copilot() -> Result<Vec<McpServer>, AppError> {
    let mut imported = BTreeMap::<String, Value>::new();
    for path in target_paths()? {
        let root = read_root(&path)?;
        if let Some(servers) = root.get("servers").and_then(Value::as_object) {
            for (id, server) in servers {
                imported.entry(id.clone()).or_insert_with(|| server.clone());
            }
        }
    }

    Ok(imported
        .into_iter()
        .map(|(id, server)| McpServer {
            name: id.clone(),
            id,
            server,
            apps: McpApps {
                copilot_byok: true,
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
    fn invalid_later_profile_leaves_every_mcp_target_unchanged() {
        let temp = tempfile::tempdir().expect("temp directory");
        let first = temp.path().join("first").join("mcp.json");
        let second = temp.path().join("second").join("mcp.json");
        fs::create_dir_all(first.parent().unwrap()).expect("first directory");
        fs::create_dir_all(second.parent().unwrap()).expect("second directory");
        let original = br#"{"servers":{"existing":{"command":"old"}}}"#;
        fs::write(&first, original).expect("first config");
        fs::write(&second, "not valid json5").expect("invalid second config");

        let result = update_paths(
            vec![first.clone(), second],
            "new",
            Some(&json!({"command": "new"})),
        );

        assert!(result.is_err());
        assert_eq!(fs::read(first).expect("first unchanged"), original);
    }
}
