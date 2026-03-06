//! Claude MCP configuration handling

use crate::app_config::McpServer;
use crate::error::AppError;

pub fn read_claude_mcp_config() -> Result<Vec<McpServer>, AppError> {
    Ok(vec![])
}

pub fn write_claude_mcp_config(_servers: &[McpServer]) -> Result<(), AppError> {
    Ok(())
}
