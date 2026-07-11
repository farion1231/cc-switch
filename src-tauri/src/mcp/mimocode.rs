//! MimoCode MCP 同步和导入模块
//!
//! MimoCode 使用与 OpenCode 相同的配置格式（local/remote），因此复用 opencode 模块的格式转换函数。

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::mimocode_config;

use super::opencode::{convert_from_opencode_format, convert_to_opencode_format};
use super::validation::validate_server_spec;

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if MimoCode MCP sync should proceed
fn should_sync_mimocode_mcp() -> bool {
    mimocode_config::get_mimocode_dir().exists()
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to MimoCode live config
pub fn sync_single_server_to_mimocode(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_mimocode_mcp() {
        return Ok(());
    }

    // MimoCode uses the same format as OpenCode (local/remote)
    let mimocode_spec = convert_to_opencode_format(server_spec)?;

    mimocode_config::set_mcp_server(id, mimocode_spec)
}

/// Remove a single MCP server from MimoCode live config
pub fn remove_server_from_mimocode(id: &str) -> Result<(), AppError> {
    if !should_sync_mimocode_mcp() {
        return Ok(());
    }

    mimocode_config::remove_mcp_server(id)
}

/// Import MCP servers from MimoCode config to unified structure
///
/// Existing servers will have MimoCode app enabled without overwriting other fields.
pub fn import_from_mimocode(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = mimocode_config::get_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in mcp_map {
        // MimoCode uses OpenCode format, reuse the same converter
        let unified_spec = match convert_from_opencode_format(&spec) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Skip invalid MimoCode MCP server '{id}': {e}");
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
            if !existing.apps.mimo {
                existing.apps.mimo = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for MimoCode");
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
                        mimo: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from MimoCode");
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
