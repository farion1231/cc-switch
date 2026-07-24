//! CodeFree MCP 同步和导入模块
//!
//! CodeFree-O 的 MCP 配置格式与 OpenCode 完全相同（JSON，`mcp` 键下），
//! 因此格式转换逻辑（stdio↔local, sse/http↔remote）可直接复用。

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::codefree_config;
use crate::error::AppError;

use super::opencode::{convert_from_opencode_format, convert_to_opencode_format};
use super::validation::validate_server_spec;

fn should_sync_codefree_mcp() -> bool {
    codefree_config::get_codefree_config_dir().exists()
}

pub fn sync_single_server_to_codefree(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_codefree_mcp() {
        return Ok(());
    }

    let codefree_spec = convert_to_opencode_format(server_spec)?;
    codefree_config::set_mcp_server(id, codefree_spec)
}

pub fn remove_server_from_codefree(id: &str) -> Result<(), AppError> {
    if !should_sync_codefree_mcp() {
        return Ok(());
    }

    codefree_config::remove_mcp_server(id)
}

pub fn import_from_codefree(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = codefree_config::get_mcp_servers()?;
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
                log::warn!("Skip invalid CodeFree MCP server '{id}': {e}");
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
            if !existing.apps.codefree {
                existing.apps.codefree = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for CodeFree");
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
                        grokbuild: false,
                        opencode: false,
                        hermes: false,
                        codefree: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from CodeFree");
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
