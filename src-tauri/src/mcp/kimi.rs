//! Kimi MCP sync and import module
//!
//! Handles conversion between CC Switch unified MCP format and Kimi mcp.json format.
//!
//! Kimi stores user-level MCP server declarations at `~/.kimi-code/mcp.json`:
//!
//! ```json
//! { "mcpServers": { "<name>": { "command": "npx", "args": [...], "env": {} } } }
//! ```
//!
//! Format mapping (Kimi infers transport from the field shape, like Hermes):
//! - stdio: `command` (+ optional `args`, `env`, `cwd`)
//! - sse/http: `url` (+ optional `headers`, `bearerTokenEnvVar`)
//!
//! See <https://www.kimi.com/code/docs/en/kimi-code-cli/customization/mcp.html>

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::kimi_config;

use super::validation::validate_server_spec;

/// Check if Kimi MCP sync should proceed
fn should_sync_kimi_mcp() -> bool {
    kimi_config::get_kimi_dir().exists()
}

// ============================================================================
// Format Conversion: CC Switch -> Kimi
// ============================================================================

/// Convert CC Switch unified format to Kimi mcp.json format.
///
/// Conversion rules:
/// - `stdio`: output `command`, `args`, `env`, `cwd` (strip `type` field)
/// - `sse`/`http`: output `url`, `headers` (strip `type` field)
fn convert_to_kimi_format(spec: &Value) -> Result<Value, AppError> {
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
            if let Some(cwd) = obj.get("cwd") {
                result.insert("cwd".into(), cwd.clone());
            }
        }
        "sse" | "http" => {
            if let Some(url) = obj.get("url") {
                result.insert("url".into(), url.clone());
            }
            if let Some(headers) = obj.get("headers") {
                if headers.is_object() && !headers.as_object().map(|o| o.is_empty()).unwrap_or(true)
                {
                    result.insert("headers".into(), headers.clone());
                }
            }
            if let Some(bearer) = obj.get("bearerTokenEnvVar") {
                result.insert("bearerTokenEnvVar".into(), bearer.clone());
            }
        }
        _ => {
            return Err(AppError::McpValidation(format!("Unknown MCP type: {typ}")));
        }
    }

    Ok(Value::Object(result))
}

// ============================================================================
// Format Conversion: Kimi -> CC Switch
// ============================================================================

/// Convert Kimi mcp.json format to CC Switch unified format.
///
/// Conversion rules:
/// - If `command` exists: set `type: "stdio"`, extract `command`, `args`, `env`, `cwd`
/// - If `url` exists: set `type: "sse"`, extract `url`, `headers`
fn convert_from_kimi_format(id: &str, spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("Kimi MCP spec must be a JSON object".into()))?;

    let mut result = serde_json::Map::new();

    if obj.contains_key("command") {
        result.insert("type".into(), json!("stdio"));
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
        if let Some(cwd) = obj.get("cwd") {
            result.insert("cwd".into(), cwd.clone());
        }
    } else if obj.contains_key("url") {
        result.insert("type".into(), json!("sse"));
        if let Some(url) = obj.get("url") {
            result.insert("url".into(), url.clone());
        }
        if let Some(headers) = obj.get("headers") {
            if headers.is_object() && !headers.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                result.insert("headers".into(), headers.clone());
            }
        }
    } else {
        return Err(AppError::McpValidation(format!(
            "Kimi MCP server '{id}' has neither 'command' nor 'url' field"
        )));
    }

    Ok(Value::Object(result))
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to Kimi live mcp.json (merge-on-write).
///
/// Strategy:
/// 1. Read existing mcpServers from mcp.json
/// 2. If the server already exists, merge: keep Kimi-specific fields, overwrite core fields
/// 3. Write back under the `mcpServers` key
pub fn sync_single_server_to_kimi(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_kimi_mcp() {
        return Ok(());
    }

    let kimi_spec = convert_to_kimi_format(server_spec)?;
    let id_owned = id.to_string();

    kimi_config::update_mcp_servers_json(|servers| {
        let merged_json = if let Some(existing) = servers.get(&id_owned) {
            merge_kimi_spec(existing, &kimi_spec)
        } else {
            kimi_spec.clone()
        };
        servers.insert(id_owned.clone(), merged_json);
        Ok(())
    })
}

/// Merge new spec into existing Kimi spec, preserving Kimi-specific fields
/// (`cwd`, `bearerTokenEnvVar`, `enabled`, ...). Core fields come from `new_spec`.
fn merge_kimi_spec(existing: &Value, new_spec: &Value) -> Value {
    let mut result = serde_json::Map::new();

    if let Some(existing_obj) = existing.as_object() {
        for (key, val) in existing_obj {
            result.insert(key.clone(), val.clone());
        }
    }
    if let Some(new_obj) = new_spec.as_object() {
        for (key, val) in new_obj {
            result.insert(key.clone(), val.clone());
        }
    }

    Value::Object(result)
}

/// Remove a single MCP server from Kimi live mcp.json.
pub fn remove_server_from_kimi(id: &str) -> Result<(), AppError> {
    if !should_sync_kimi_mcp() {
        return Ok(());
    }

    let id_owned = id.to_string();
    kimi_config::update_mcp_servers_json(|servers| {
        servers.remove(&id_owned);
        Ok(())
    })
}

/// Import MCP servers from Kimi mcp.json to the unified structure.
///
/// Existing servers get the Kimi app enabled without overwriting other fields.
pub fn import_from_kimi(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let kimi_servers = kimi_config::get_mcp_servers_json()?;
    if kimi_servers.is_empty() {
        return Ok(0);
    }

    // Ensure servers map exists
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;

    for (id, spec) in kimi_servers {
        let unified_spec = match convert_from_kimi_format(&id, &spec) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Skip invalid Kimi MCP server '{id}': {e}");
                continue;
            }
        };

        if let Err(e) = validate_server_spec(&unified_spec) {
            log::warn!("Skip invalid MCP server '{id}' after conversion: {e}");
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            // Existing server: just enable the Kimi app
            if !existing.apps.kimi {
                existing.apps.kimi = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for Kimi");
            }
        } else {
            // New server: default to only Kimi enabled
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
                        kimi: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported MCP server '{id}' from Kimi");
        }
    }

    Ok(changed)
}
