//! Cursor MCP sync and import module
//!
//! Handles conversion between CC Switch unified MCP format and Cursor mcp.json format.
//!
//! ## Format mapping
//!
//! Cursor uses the standard VS Code MCP format (`.cursor/mcp.json`):
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "server-id": {
//!       "command": "npx",
//!       "args": ["-y", "@modelcontextprotocol/server-filesystem"],
//!       "env": { "KEY": "value" }
//!     }
//!   }
//! }
//! ```
//!
//! CC Switch unified format is already compatible with this structure,
//! so no format conversion is needed.

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::cursor_config;
use crate::error::AppError;

use super::validation::validate_server_spec;

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if Cursor MCP sync should proceed
fn should_sync_cursor_mcp() -> bool {
    cursor_config::get_cursor_dir().exists()
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to Cursor live config
pub fn sync_single_server_to_cursor(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_cursor_mcp() {
        return Ok(());
    }

    // Cursor uses the same format as CC Switch unified format,
    // so we use the server spec directly.
    let mut updated = cursor_config::read_mcp_servers_map()?;
    updated.insert(id.to_string(), server_spec.clone());
    cursor_config::set_mcp_servers_map(&updated)
}

/// Remove a single MCP server from Cursor live config
pub fn remove_server_from_cursor(id: &str) -> Result<(), AppError> {
    if !should_sync_cursor_mcp() {
        return Ok(());
    }

    let mut current = cursor_config::read_mcp_servers_map()?;
    current.remove(id);
    cursor_config::set_mcp_servers_map(&current)
}

/// Import MCP servers from Cursor config to unified structure
///
/// Existing servers will have Cursor app enabled without overwriting other fields.
pub fn import_from_cursor(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = cursor_config::read_mcp_servers_map()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    // Ensure servers map exists
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut count = 0;
    for (id, spec) in mcp_map {
        if let Err(e) = validate_server_spec(&spec) {
            log::warn!("Skip invalid Cursor MCP server '{id}': {e}");
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            // Server already exists — enable Cursor without overwriting
            if !existing.apps.cursor {
                existing.apps.cursor = true;
                count += 1;
            }
        } else {
            // New server: default to only Cursor enabled
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
                        cursor: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            count += 1;
        }
    }

    Ok(count)
}
