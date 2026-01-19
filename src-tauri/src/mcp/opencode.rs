//! OpenCode MCP 同步和导入模块
//!
//! 本模块处理 CC Switch 统一 MCP 格式与 OpenCode 格式之间的转换。
//!
//! ## 格式差异
//!
//! | CC Switch 统一格式    | OpenCode 格式       |
//! |----------------------|---------------------|
//! | `type: "stdio"`      | `type: "local"`     |
//! | `command` + `args`   | `command: [cmd, ...args]` |
//! | `env`                | `environment`       |
//! | `type: "sse"/"http"` | `type: "remote"`    |
//! | `url`                | `url`               |

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::opencode_config;

use super::validation::validate_server_spec;

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if OpenCode MCP sync should proceed
fn should_sync_opencode_mcp() -> bool {
    // Skip if OpenCode config directory doesn't exist
    opencode_config::get_opencode_dir().exists()
}

// ============================================================================
// Format Conversion: CC Switch → OpenCode
// ============================================================================

/// Convert CC Switch unified format to OpenCode format
///
/// Conversion rules:
/// - `stdio` → `local`, command+args → command array, env → environment
/// - `sse`/`http` → `remote`, url preserved
pub fn convert_to_opencode_format(spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP spec must be a JSON object".into()))?;

    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");

    let mut result = serde_json::Map::new();

    match typ {
        "stdio" => {
            // Convert to "local" type
            result.insert("type".into(), json!("local"));

            // Merge command and args into a single array
            let cmd = obj.get("command").and_then(|v| v.as_str()).unwrap_or("");
            let mut command_arr = vec![json!(cmd)];

            if let Some(args) = obj.get("args").and_then(|v| v.as_array()) {
                for arg in args {
                    command_arr.push(arg.clone());
                }
            }
            result.insert("command".into(), Value::Array(command_arr));

            // Convert env → environment
            if let Some(env) = obj.get("env") {
                if env.is_object() && !env.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                    result.insert("environment".into(), env.clone());
                }
            }

            // Add enabled flag (OpenCode expects this)
            result.insert("enabled".into(), json!(true));
        }
        "sse" | "http" => {
            // Convert to "remote" type
            result.insert("type".into(), json!("remote"));

            // Preserve url
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }

            // Convert headers if present
            if let Some(headers) = obj.get("headers") {
                if headers.is_object() && !headers.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    result.insert("headers".into(), headers.clone());
                }
            }

            // Add enabled flag
            result.insert("enabled".into(), json!(true));
        }
        _ => {
            return Err(AppError::McpValidation(format!("Unknown MCP type: {typ}")));
        }
    }

    Ok(Value::Object(result))
}

// ============================================================================
// Format Conversion: OpenCode → CC Switch
// ============================================================================

/// Convert OpenCode format to CC Switch unified format
///
/// Conversion rules:
/// - `local` → `stdio`, command array → command+args, environment → env
/// - `remote` → `sse`, url preserved
pub fn convert_from_opencode_format(spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("OpenCode MCP spec must be a JSON object".into()))?;

    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("local");

    let mut result = serde_json::Map::new();

    match typ {
        "local" => {
            // Convert to "stdio" type
            result.insert("type".into(), json!("stdio"));

            // Split command array into command and args
            if let Some(cmd_arr) = obj.get("command").and_then(|v| v.as_array()) {
                if !cmd_arr.is_empty() {
                    // First element is the command
                    if let Some(cmd) = cmd_arr.first().and_then(|v| v.as_str()) {
                        result.insert("command".into(), json!(cmd));
                    }

                    // Rest are args
                    if cmd_arr.len() > 1 {
                        let args: Vec<Value> = cmd_arr[1..].to_vec();
                        result.insert("args".into(), Value::Array(args));
                    }
                }
            }

            // Convert environment → env
            if let Some(env) = obj.get("environment") {
                if env.is_object() && !env.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                    result.insert("env".into(), env.clone());
                }
            }
        }
        "remote" => {
            // Convert to "sse" type (default remote protocol)
            result.insert("type".into(), json!("sse"));

            // Preserve url
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }

            // Preserve headers
            if let Some(headers) = obj.get("headers") {
                if headers.is_object() && !headers.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    result.insert("headers".into(), headers.clone());
                }
            }
        }
        _ => {
            return Err(AppError::McpValidation(format!(
                "Unknown OpenCode MCP type: {typ}"
            )));
        }
    }

    Ok(Value::Object(result))
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to OpenCode live config
pub fn sync_single_server_to_opencode(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_opencode_mcp() {
        return Ok(());
    }

    // Convert to OpenCode format
    let opencode_spec = convert_to_opencode_format(server_spec)?;

    // Set in OpenCode config
    opencode_config::set_mcp_server(id, opencode_spec)
}

/// Remove a single MCP server from OpenCode live config
pub fn remove_server_from_opencode(id: &str) -> Result<(), AppError> {
    if !should_sync_opencode_mcp() {
        return Ok(());
    }

    opencode_config::remove_mcp_server(id)
}

/// Import MCP servers from OpenCode config to unified structure
///
/// Existing servers will have OpenCode app enabled without overwriting other fields.
pub fn import_from_opencode(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = opencode_config::get_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    // Ensure servers map exists
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in mcp_map {
        // Convert from OpenCode format to unified format
        let unified_spec = match convert_from_opencode_format(&spec) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Skip invalid OpenCode MCP server '{id}': {e}");
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };

        // Validate the converted spec
        if let Err(e) = validate_server_spec(&unified_spec) {
            log::warn!("Skip invalid MCP server '{id}' after conversion: {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            // Existing server: just enable OpenCode app
            if !existing.apps.opencode {
                existing.apps.opencode = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for OpenCode");
            }
        } else {
            // New server: default to only OpenCode enabled
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
                        opencode: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from OpenCode");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_convert_stdio_to_local() {
        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": { "HOME": "/Users/test" }
        });

        let result = convert_to_opencode_format(&spec).unwrap();
        assert_eq!(result["type"], "local");
        assert_eq!(result["command"][0], "npx");
        assert_eq!(result["command"][1], "-y");
        assert_eq!(
            result["command"][2],
            "@modelcontextprotocol/server-filesystem"
        );
        assert_eq!(result["environment"]["HOME"], "/Users/test");
        assert_eq!(result["enabled"], true);
    }

    #[test]
    fn test_convert_sse_to_remote() {
        let spec = json!({
            "type": "sse",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer xxx" }
        });

        let result = convert_to_opencode_format(&spec).unwrap();
        assert_eq!(result["type"], "remote");
        assert_eq!(result["url"], "https://example.com/mcp");
        assert_eq!(result["headers"]["Authorization"], "Bearer xxx");
        assert_eq!(result["enabled"], true);
    }

    #[test]
    fn test_convert_local_to_stdio() {
        let spec = json!({
            "type": "local",
            "command": ["npx", "-y", "@modelcontextprotocol/server-filesystem"],
            "environment": { "HOME": "/Users/test" }
        });

        let result = convert_from_opencode_format(&spec).unwrap();
        assert_eq!(result["type"], "stdio");
        assert_eq!(result["command"], "npx");
        assert_eq!(result["args"][0], "-y");
        assert_eq!(result["args"][1], "@modelcontextprotocol/server-filesystem");
        assert_eq!(result["env"]["HOME"], "/Users/test");
    }

    #[test]
    fn test_convert_remote_to_sse() {
        let spec = json!({
            "type": "remote",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer xxx" }
        });

        let result = convert_from_opencode_format(&spec).unwrap();
        assert_eq!(result["type"], "sse");
        assert_eq!(result["url"], "https://example.com/mcp");
        assert_eq!(result["headers"]["Authorization"], "Bearer xxx");
    }
}
