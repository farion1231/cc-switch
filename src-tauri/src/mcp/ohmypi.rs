//! Oh My Pi MCP 同步和导入模块
//!
//! Oh My Pi's `mcp.json` uses the standard `mcpServers` map, matching CC
//! Switch's unified MCP format, so sync is a pass-through.

use super::validation::validate_server_spec;
use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use serde_json::Value;
use std::collections::HashMap;

/// Sync a single MCP server to Oh My Pi's `mcp.json`.
pub fn sync_single_server_to_ohmypi(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    crate::ohmypi_config::set_ohmypi_mcp_server(id, server_spec)
}

/// Remove a single MCP server from Oh My Pi's `mcp.json`.
pub fn remove_server_from_ohmypi(id: &str) -> Result<(), AppError> {
    crate::ohmypi_config::remove_ohmypi_mcp_server(id)
}

/// Import MCP servers from Oh My Pi's `mcp.json` into the unified structure.
///
/// Existing servers will have Oh My Pi enabled without overwriting other fields.
pub fn import_from_ohmypi(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = crate::ohmypi_config::read_ohmypi_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in mcp_map {
        if let Err(e) = validate_server_spec(&spec) {
            log::warn!("Skip invalid Oh My Pi MCP server '{id}': {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            if !existing.apps.ohmypi {
                existing.apps.ohmypi = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for Oh My Pi");
            }
        } else {
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec,
                    apps: McpApps {
                        claude: false,
                        codex: false,
                        gemini: false,
                        grokbuild: false,
                        opencode: false,
                        hermes: false,
                        ohmypi: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from Oh My Pi");
        }
    }

    if !errors.is_empty() {
        log::warn!(
            "Import completed with {} failures: {:?}",
            errors.len(),
            errors
        );
    }

    Ok(changed)
}
