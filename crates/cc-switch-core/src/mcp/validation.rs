//! MCP configuration validation

use crate::error::AppError;

pub fn validate_mcp_config(_config: &serde_json::Value) -> Result<(), AppError> {
    Ok(())
}
