//! MiMo Code MCP sync and import support.

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::mimocode_config;

use super::opencode::{convert_from_opencode_format, convert_to_opencode_format};
use super::validation::validate_server_spec;

fn should_sync_mimo_mcp() -> bool {
    mimocode_config::get_mimo_config_path().exists() || mimocode_config::get_mimo_dir().exists()
}

pub fn sync_single_server_to_mimo(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_mimo_mcp() {
        return Ok(());
    }

    let mimo_spec = convert_to_opencode_format(server_spec)?;
    mimocode_config::set_mcp_server(id, mimo_spec)
}

pub fn remove_server_from_mimo(id: &str) -> Result<(), AppError> {
    if !should_sync_mimo_mcp() {
        return Ok(());
    }

    mimocode_config::remove_mcp_server(id)
}

pub fn import_from_mimo(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = mimocode_config::get_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in mcp_map {
        let unified_spec = match convert_from_opencode_format(&spec) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Skip invalid MiMo Code MCP server '{id}': {e}");
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };

        if let Err(e) = validate_server_spec(&unified_spec) {
            log::warn!("Skip invalid MiMo Code MCP server '{id}' after conversion: {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            if !existing.apps.mimo {
                existing.apps.mimo = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for MiMo Code");
            }
        } else {
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: unified_spec,
                    apps: McpApps {
                        claude: false,
                        codex: false,
                        gemini: false,
                        opencode: false,
                        mimo: true,
                        hermes: false,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from MiMo Code");
        }
    }

    if !errors.is_empty() {
        log::warn!(
            "MiMo Code MCP import completed with {} failures: {:?}",
            errors.len(),
            errors
        );
    }

    Ok(changed)
}
