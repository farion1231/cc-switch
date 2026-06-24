//! Kimi Code MCP sync and import module.
//!
//! Kimi Code stores user-level MCP as `{ "mcpServers": { "<id>": { ... } } }`.

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::kimi_config;

use super::validation::validate_server_spec;

fn should_sync_kimi_mcp() -> bool {
    kimi_config::get_kimi_dir().exists() || kimi_config::get_kimi_mcp_path().exists()
}

pub fn sync_single_server_to_kimi(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_kimi_mcp() {
        return Ok(());
    }

    validate_server_spec(server_spec)?;
    kimi_config::set_mcp_server(id, server_spec.clone())
}

pub fn remove_server_from_kimi(id: &str) -> Result<(), AppError> {
    if !should_sync_kimi_mcp() {
        return Ok(());
    }

    kimi_config::remove_mcp_server(id)
}

pub fn import_from_kimi(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let map = kimi_config::get_mcp_servers()?;
    if map.is_empty() {
        return Ok(0);
    }

    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);
    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in map.iter() {
        if let Err(e) = validate_server_spec(spec) {
            log::warn!("Skip invalid Kimi MCP server '{id}': {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(id) {
            if !existing.apps.kimi {
                existing.apps.kimi = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for Kimi");
            }
        } else {
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec.clone(),
                    apps: McpApps {
                        claude: false,
                        codex: false,
                        gemini: false,
                        opencode: false,
                        kimi: true,
                        hermes: false,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from Kimi");
        }
    }

    if !errors.is_empty() {
        log::warn!(
            "Kimi MCP import completed with {} failures: {:?}",
            errors.len(),
            errors
        );
    }

    Ok(changed)
}
