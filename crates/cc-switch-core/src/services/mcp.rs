//! MCP service - business logic for MCP server management

use indexmap::IndexMap;

use crate::app_config::{AppType, McpApps, McpServer};
use crate::error::AppError;
use crate::store::AppState;

/// MCP business logic service
pub struct McpService;

impl McpService {
    /// Get all MCP servers
    pub fn get_all_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
        state.db.get_all_mcp_servers()
    }

    /// Get a single MCP server
    pub fn get_server(state: &AppState, id: &str) -> Result<Option<McpServer>, AppError> {
        state.db.get_mcp_server(id)
    }

    /// Save or update an MCP server
    pub fn save_server(state: &AppState, server: &McpServer) -> Result<(), AppError> {
        state.db.save_mcp_server(server)
    }

    /// Delete an MCP server
    pub fn delete_server(state: &AppState, id: &str) -> Result<(), AppError> {
        state.db.delete_mcp_server(id)
    }

    /// Toggle MCP server for an app
    pub fn toggle_app(
        state: &AppState,
        id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let mut server = state
            .db
            .get_mcp_server(id)?
            .ok_or_else(|| AppError::Message(format!("MCP server {} not found", id)))?;

        server.apps.set_enabled_for(&app, enabled);
        state.db.save_mcp_server(&server)
    }

    /// Import MCP servers from Claude config
    pub fn import_from_claude(state: &AppState) -> Result<usize, AppError> {
        let claude_path = crate::config::get_default_claude_mcp_path();
        if !claude_path.exists() {
            return Ok(0);
        }

        let content = std::fs::read_to_string(&claude_path)
            .map_err(|e| crate::error::AppError::io(&claude_path, e))?;
        let config: serde_json::Value = serde_json::from_str(&content)
            .map_err(|e| crate::error::AppError::json(&claude_path, e))?;

        let servers = config
            .get("mcpServers")
            .and_then(|v| v.as_object())
            .ok_or_else(|| AppError::Message("No mcpServers found in Claude config".to_string()))?;

        let mut imported = 0;
        for (id, server_config) in servers {
            if state.db.get_mcp_server(id)?.is_some() {
                continue;
            }

            let server = McpServer {
                id: id.clone(),
                name: id.clone(),
                server: server_config.clone(),
                apps: McpApps {
                    claude: true,
                    codex: false,
                    gemini: false,
                    opencode: false,
                },
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            };

            state.db.save_mcp_server(&server)?;
            imported += 1;
        }

        Ok(imported)
    }

    /// Sync all enabled MCP servers to app configs
    pub fn sync_all_enabled(_state: &AppState) -> Result<(), AppError> {
        Ok(())
    }
}
