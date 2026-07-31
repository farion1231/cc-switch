//! qoder CLI (qodercli) MCP 同步和导入模块
//!
//! 本模块处理 CC Switch 统一 MCP 格式与 qoder CLI 格式之间的转换。
//!
//! qoder CLI 1.1.x 的用户级 MCP 服务器存于 `~/.qoder/settings.json` 的
//! `mcpServers` map（与 `qodercli mcp add -s user` 写入的结构一致）。
//!
//! ## 格式差异
//!
//! | CC Switch 统一格式    | qoder CLI 格式          |
//! |----------------------|-------------------------|
//! | `type: "stdio"`      | 无 `type` 字段           |
//! | `command` + `args`   | `command` + `args`（同） |
//! | `env`                | `env`（同）              |
//! | `type: "sse"/"http"` | `type: "sse"/"http"`（同）|
//! | `url` / `headers`    | `url` / `headers`（同）  |

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::qodercli_config;

use super::validation::validate_server_spec;

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if qodercli MCP sync should proceed
fn should_sync_qodercli_mcp() -> bool {
    // Skip if qoder CLI config directory doesn't exist
    qodercli_config::get_qodercli_dir().exists()
}

// ============================================================================
// Format Conversion: CC Switch → qoder CLI
// ============================================================================

/// Convert CC Switch unified format to qoder CLI format
///
/// Conversion rules:
/// - `stdio` → drop the `type` field, keep command/args/env as-is
/// - `sse`/`http` → keep type/url/headers as-is
pub fn convert_to_qodercli_format(spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("MCP spec must be a JSON object".into()))?;

    let typ = obj.get("type").and_then(|v| v.as_str()).unwrap_or("stdio");

    let mut result = serde_json::Map::new();

    match typ {
        "stdio" => {
            if let Some(command) = obj.get("command") {
                result.insert("command".into(), command.clone());
            }
            if let Some(args) = obj.get("args") {
                if args.is_array() && !args.as_array().map(|a| a.is_empty()).unwrap_or(true) {
                    result.insert("args".into(), args.clone());
                }
            }
            if let Some(env) = obj.get("env") {
                if env.is_object() && !env.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                    result.insert("env".into(), env.clone());
                }
            }
        }
        "sse" | "http" => {
            result.insert("type".into(), json!(typ));
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers") {
                if headers.is_object() && !headers.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    result.insert("headers".into(), headers.clone());
                }
            }
        }
        _ => {
            return Err(AppError::McpValidation(format!("Unknown MCP type: {typ}")));
        }
    }

    Ok(Value::Object(result))
}

// ============================================================================
// Format Conversion: qoder CLI → CC Switch
// ============================================================================

/// Convert qoder CLI format to CC Switch unified format
///
/// Conversion rules:
/// - no `type` field → `stdio`
/// - `type: "sse"/"http"` → preserved
pub fn convert_from_qodercli_format(spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("qodercli MCP spec must be a JSON object".into()))?;

    let typ = obj.get("type").and_then(|v| v.as_str());

    let mut result = serde_json::Map::new();

    match typ {
        None => {
            result.insert("type".into(), json!("stdio"));
            if let Some(command) = obj.get("command") {
                result.insert("command".into(), command.clone());
            }
            if let Some(args) = obj.get("args") {
                result.insert("args".into(), args.clone());
            }
            if let Some(env) = obj.get("env") {
                result.insert("env".into(), env.clone());
            }
        }
        Some("sse") | Some("http") => {
            result.insert("type".into(), json!(typ.unwrap()));
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers") {
                result.insert("headers".into(), headers.clone());
            }
        }
        Some(other) => {
            return Err(AppError::McpValidation(format!(
                "Unknown qodercli MCP type: {other}"
            )));
        }
    }

    Ok(Value::Object(result))
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to qoder CLI live config (`~/.qoder/settings.json`)
pub fn sync_single_server_to_qodercli(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_qodercli_mcp() {
        return Ok(());
    }

    let qodercli_spec = convert_to_qodercli_format(server_spec)?;
    qodercli_config::set_mcp_server(id, qodercli_spec)
}

/// Remove a single MCP server from qoder CLI live config
pub fn remove_server_from_qodercli(id: &str) -> Result<(), AppError> {
    if !should_sync_qodercli_mcp() {
        return Ok(());
    }

    qodercli_config::remove_mcp_server(id)
}

/// Import MCP servers from qoder CLI config to unified structure
///
/// Existing servers will have qodercli app enabled without overwriting other fields.
pub fn import_from_qodercli(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = qodercli_config::get_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    // Ensure servers map exists
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in mcp_map {
        let unified_spec = match convert_from_qodercli_format(&spec) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Skip invalid qodercli MCP server '{id}': {e}");
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
            // Existing server: just enable qodercli app
            if !existing.apps.qodercli {
                existing.apps.qodercli = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for qodercli");
            }
        } else {
            // New server: default to only qodercli enabled
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
                        qodercli: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from qodercli");
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
    fn test_convert_stdio_drops_type() {
        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": { "HOME": "/Users/test" }
        });

        let result = convert_to_qodercli_format(&spec).unwrap();
        assert!(result.get("type").is_none());
        assert_eq!(result["command"], "npx");
        assert_eq!(result["args"][0], "-y");
        assert_eq!(result["env"]["HOME"], "/Users/test");
    }

    #[test]
    fn test_convert_http_keeps_type() {
        let spec = json!({
            "type": "http",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer xxx" }
        });

        let result = convert_to_qodercli_format(&spec).unwrap();
        assert_eq!(result["type"], "http");
        assert_eq!(result["url"], "https://example.com/mcp");
        assert_eq!(result["headers"]["Authorization"], "Bearer xxx");
    }

    #[test]
    fn test_convert_from_no_type_to_stdio() {
        let spec = json!({
            "command": "npx",
            "args": ["-y", "server-everything"]
        });

        let result = convert_from_qodercli_format(&spec).unwrap();
        assert_eq!(result["type"], "stdio");
        assert_eq!(result["command"], "npx");
        assert_eq!(result["args"][0], "-y");
    }

    #[test]
    fn test_convert_from_sse() {
        let spec = json!({
            "type": "sse",
            "url": "https://example.com/sse"
        });

        let result = convert_from_qodercli_format(&spec).unwrap();
        assert_eq!(result["type"], "sse");
        assert_eq!(result["url"], "https://example.com/sse");
    }

    #[test]
    fn test_convert_unknown_type_errors() {
        let spec = json!({ "type": "ws", "url": "wss://x" });
        assert!(convert_to_qodercli_format(&spec).is_err());
        assert!(convert_from_qodercli_format(&spec).is_err());
    }
}
