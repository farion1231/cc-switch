//! Codex MCP configuration handling

use crate::app_config::McpServer;
use crate::error::AppError;

pub fn read_codex_mcp_config() -> Result<Vec<McpServer>, AppError> {
    Ok(vec![])
}

pub fn write_codex_mcp_config(_servers: &[McpServer]) -> Result<(), AppError> {
    Ok(())
}
