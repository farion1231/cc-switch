//! Kilo MCP 同步和导入模块
//!
//! Kilo 基于 OpenCode，MCP 格式转换逻辑与 OpenCode 完全一致。

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::kilo_config;

use super::opencode::{convert_from_opencode_format, convert_to_opencode_format};
use super::validation::validate_server_spec;

fn should_sync_kilo_mcp() -> bool {
    kilo_config::get_kilo_dir().exists()
}

/// Sync a single MCP server to Kilo live config
pub fn sync_single_server_to_kilo(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_kilo_mcp() {
        return Ok(());
    }

    let kilo_spec = convert_to_opencode_format(server_spec)?;
    kilo_config::set_mcp_server(id, kilo_spec)
}

/// Remove a single MCP server from Kilo live config
pub fn remove_server_from_kilo(id: &str) -> Result<(), AppError> {
    if !should_sync_kilo_mcp() {
        return Ok(());
    }

    kilo_config::remove_mcp_server(id)
}

/// Import MCP servers from Kilo config to unified structure
#[allow(dead_code)]
pub fn import_from_kilo(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = kilo_config::get_mcp_servers()?;
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
                log::warn!("Skip invalid Kilo MCP server '{id}': {e}");
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };

        if let Err(e) = validate_server_spec(&unified_spec) {
            log::warn!("Skip invalid MCP server '{id}' after conversion: {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            if !existing.apps.kilo {
                existing.apps.kilo = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for Kilo");
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
                        hermes: false,
                        kilo: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from Kilo");
        }
    }

    if !errors.is_empty() {
        log::warn!(
            "Kilo MCP import completed with {} failures: {:?}",
            errors.len(),
            errors
        );
    }

    Ok(changed)
}
