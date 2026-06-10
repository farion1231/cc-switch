use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync() -> bool {
    crate::antigravity_config::get_antigravity_dir().exists()
}

fn collect_enabled_servers(config: &McpConfig) -> HashMap<String, Value> {
    config
        .servers
        .iter()
        .filter_map(|(id, entry)| {
            entry
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
                .then(|| {
                    extract_server_spec(entry)
                        .ok()
                        .map(|spec| (id.clone(), spec))
                })
                .flatten()
        })
        .collect()
}

pub fn sync_enabled_to_antigravity(config: &MultiAppConfig) -> Result<(), AppError> {
    if should_sync() {
        crate::antigravity_mcp::set_mcp_servers_map(&collect_enabled_servers(
            &config.mcp.antigravity,
        ))?;
    }
    Ok(())
}

pub fn import_from_antigravity(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let imported = crate::antigravity_mcp::read_mcp_servers_map()?;
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);
    let mut changed = 0;

    for (id, spec) in imported {
        if let Err(error) = validate_server_spec(&spec) {
            log::warn!("Skipping invalid Antigravity MCP server '{id}': {error}");
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            if !existing.apps.antigravity {
                existing.apps.antigravity = true;
                changed += 1;
            }
        } else {
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id,
                    server: spec,
                    apps: McpApps {
                        antigravity: true,
                        ..Default::default()
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
        }
    }

    Ok(changed)
}

pub fn sync_single_server_to_antigravity(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync() {
        return Ok(());
    }
    let mut servers = crate::antigravity_mcp::read_mcp_servers_map()?;
    servers.insert(id.to_string(), server_spec.clone());
    crate::antigravity_mcp::set_mcp_servers_map(&servers)
}

pub fn remove_server_from_antigravity(id: &str) -> Result<(), AppError> {
    if !should_sync() {
        return Ok(());
    }
    let mut servers = crate::antigravity_mcp::read_mcp_servers_map()?;
    servers.remove(id);
    crate::antigravity_mcp::set_mcp_servers_map(&servers)
}
