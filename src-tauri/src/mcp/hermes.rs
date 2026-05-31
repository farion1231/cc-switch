//! Hermes MCP sync and import module
//!
//! Handles conversion between CC Switch unified MCP format and Hermes config.yaml format.
//!
//! ## Format mapping
//!
//! | CC Switch unified (JSON)                        | Hermes config.yaml (YAML)       |
//! |-------------------------------------------------|---------------------------------|
//! | `{"type":"stdio","command":"npx","args":[...],"env":{}}` | `command: npx`, `args: [...]`, `env: {}` |
//! | `{"type":"sse"/"http","url":"...","headers":{}}` | `url: "..."`, `headers: {}`    |
//!
//! Key differences from Claude format:
//! - Hermes has NO explicit `type` field -- it infers stdio (has `command`) vs HTTP (has `url`)
//! - Hermes has extra fields: `enabled`, `timeout`, `connect_timeout`, `tools`, `sampling`
//! - These Hermes-specific fields are preserved on merge-on-write and stripped on import

use serde_json::{json, Value};
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::hermes_config;

use super::validation::validate_server_spec;

/// Hermes-specific fields preserved on merge-on-write, stripped on import.
/// Update this list when Hermes adds new per-server config fields.
///
/// `auth` ("oauth" / absent) is an OAuth-type declaration read by Hermes —
/// CC Switch has no OAuth UI, but losing the field on round-trip downgrades
/// the server to unauthenticated calls.
const HERMES_EXTRA_FIELDS: &[&str] = &[
    "enabled",
    "timeout",
    "connect_timeout",
    "tools",
    "sampling",
    "roots",
    "auth",
];

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if Hermes MCP sync should proceed
fn should_sync_hermes_mcp() -> bool {
    hermes_config::get_hermes_dir().exists()
}

// ============================================================================
// Format Conversion: CC Switch -> Hermes
// ============================================================================

/// Convert CC Switch unified format to Hermes format
///
/// Conversion rules:
/// - `stdio`: output `command`, `args`, `env` (strip `type` field)
/// - `sse`/`http`: output `url`, `headers` (strip `type` field)
/// - Always add `enabled: true`
fn convert_to_hermes_format(spec: &Value) -> Result<Value, AppError> {
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

    result.insert("enabled".into(), json!(true));

    Ok(Value::Object(result))
}

// ============================================================================
// Format Conversion: Hermes -> CC Switch
// ============================================================================

/// Convert Hermes format to CC Switch unified format
///
/// Conversion rules:
/// - If `command` exists: set `type: "stdio"`, extract `command`, `args`, `env`
/// - If `url` exists: set `type: "sse"`, extract `url`, `headers`
/// - Strip Hermes-specific fields: `enabled`, `timeout`, `connect_timeout`, `tools`, `sampling`
fn convert_from_hermes_format(id: &str, spec: &Value) -> Result<Value, AppError> {
    let obj = spec
        .as_object()
        .ok_or_else(|| AppError::McpValidation("Hermes MCP spec must be a JSON object".into()))?;

    let mut result = serde_json::Map::new();

    if obj.contains_key("command") {
        // stdio type
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
    } else if obj.contains_key("url") {
        // HTTP/SSE type
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
            "Hermes MCP server '{id}' has neither 'command' nor 'url' field"
        )));
    }

    // Note: Hermes-specific fields (enabled, timeout, connect_timeout, tools, sampling)
    // are intentionally NOT copied -- they are stripped on import.

    Ok(Value::Object(result))
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to Hermes live config (merge-on-write)
///
/// Strategy:
/// 1. Read existing mcp_servers from config.yaml
/// 2. If server already exists, merge: keep Hermes-specific fields, overwrite core fields
/// 3. Set `enabled: true`
/// 4. Write back
pub fn sync_single_server_to_hermes(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_hermes_mcp() {
        return Ok(());
    }

    let hermes_spec = convert_to_hermes_format(server_spec)?;
    let id_owned = id.to_string();

    hermes_config::update_mcp_servers_yaml(|servers| {
        let id_yaml = serde_yaml::Value::String(id_owned.clone());

        let merged_json = if let Some(existing_yaml) = servers.get(&id_yaml) {
            let existing_json = hermes_config::yaml_to_json(existing_yaml)?;
            merge_hermes_spec(&existing_json, &hermes_spec)
        } else {
            hermes_spec.clone()
        };

        let merged_yaml_value = hermes_config::json_to_yaml(&merged_json)?;
        servers.insert(id_yaml, merged_yaml_value);
        Ok(())
    })
}

/// Merge new spec into existing Hermes spec, preserving Hermes-specific fields.
///
/// Core fields (command, args, env, url, headers) come from `new_spec`.
/// Hermes-specific fields (enabled, tools, sampling, etc.) are kept from
/// `existing` — this prevents CC Switch from overwriting user customizations.
fn merge_hermes_spec(existing: &Value, new_spec: &Value) -> Value {
    let mut result = serde_json::Map::new();

    // Copy Hermes-specific fields from existing config
    if let Some(existing_obj) = existing.as_object() {
        for &field in HERMES_EXTRA_FIELDS {
            if let Some(val) = existing_obj.get(field) {
                result.insert(field.to_string(), val.clone());
            }
        }
    }

    // Overwrite with core fields from new spec; for Hermes-specific fields,
    // only apply from new_spec if existing didn't already have them
    if let Some(new_obj) = new_spec.as_object() {
        for (key, val) in new_obj {
            if HERMES_EXTRA_FIELDS.contains(&key.as_str()) && result.contains_key(key) {
                continue; // Existing Hermes-specific field takes precedence
            }
            result.insert(key.clone(), val.clone());
        }
    }

    Value::Object(result)
}

/// Remove a single MCP server from Hermes live config
pub fn remove_server_from_hermes(id: &str) -> Result<(), AppError> {
    if !should_sync_hermes_mcp() {
        return Ok(());
    }

    let id_owned = id.to_string();
    hermes_config::update_mcp_servers_yaml(|servers| {
        servers.remove(serde_yaml::Value::String(id_owned.clone()));
        Ok(())
    })
}

/// Import MCP servers from Hermes config to unified structure
///
/// Existing servers will have Hermes app enabled without overwriting other fields.
pub fn import_from_hermes(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let yaml_map = hermes_config::get_mcp_servers_yaml()?;
    if yaml_map.is_empty() {
        return Ok(0);
    }

    // Ensure servers map exists
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (key, spec_yaml) in &yaml_map {
        let id = match key.as_str() {
            Some(s) => s.to_string(),
            None => {
                log::warn!("Skip Hermes MCP server with non-string key");
                continue;
            }
        };

        // Convert YAML value to JSON
        let spec_json = match hermes_config::yaml_to_json(spec_yaml) {
            Ok(j) => j,
            Err(e) => {
                log::warn!("Skip Hermes MCP server '{id}': failed to convert YAML to JSON: {e}");
                errors.push(format!("{id}: {e}"));
                continue;
            }
        };

        // Convert from Hermes format to unified format
        let unified_spec = match convert_from_hermes_format(&id, &spec_json) {
            Ok(s) => s,
            Err(e) => {
                log::warn!("Skip invalid Hermes MCP server '{id}': {e}");
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
            // Existing server: just enable Hermes app
            if !existing.apps.hermes {
                existing.apps.hermes = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for Hermes");
            }
        } else {
            // New server: default to only Hermes enabled
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
                        hermes: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from Hermes");
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

    // ========================================================================
    // convert_to_hermes_format tests
    // ========================================================================

    #[test]
    fn test_convert_stdio_to_hermes() {
        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": { "HOME": "/Users/test" }
        });

        let result = convert_to_hermes_format(&spec).unwrap();
        // No type field in Hermes format
        assert!(result.get("type").is_none());
        assert_eq!(result["command"], "npx");
        assert_eq!(result["args"][0], "-y");
        assert_eq!(result["args"][1], "@modelcontextprotocol/server-filesystem");
        assert_eq!(result["env"]["HOME"], "/Users/test");
        assert_eq!(result["enabled"], true);
    }

    #[test]
    fn test_convert_http_to_hermes() {
        let spec = json!({
            "type": "sse",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer xxx" }
        });

        let result = convert_to_hermes_format(&spec).unwrap();
        assert!(result.get("type").is_none());
        assert_eq!(result["url"], "https://example.com/mcp");
        assert_eq!(result["headers"]["Authorization"], "Bearer xxx");
        assert_eq!(result["enabled"], true);
    }

    #[test]
    fn test_convert_http_type_to_hermes() {
        let spec = json!({
            "type": "http",
            "url": "https://example.com/mcp"
        });

        let result = convert_to_hermes_format(&spec).unwrap();
        assert!(result.get("type").is_none());
        assert_eq!(result["url"], "https://example.com/mcp");
        assert_eq!(result["enabled"], true);
    }

    #[test]
    fn test_convert_stdio_empty_env_to_hermes() {
        let spec = json!({
            "type": "stdio",
            "command": "node",
            "args": [],
            "env": {}
        });

        let result = convert_to_hermes_format(&spec).unwrap();
        assert_eq!(result["command"], "node");
        // Empty args and env should be omitted
        assert!(result.get("args").is_none());
        assert!(result.get("env").is_none());
        assert_eq!(result["enabled"], true);
    }

    #[test]
    fn test_convert_unknown_type_to_hermes_fails() {
        let spec = json!({ "type": "grpc", "command": "foo" });
        assert!(convert_to_hermes_format(&spec).is_err());
    }

    // ========================================================================
    // convert_from_hermes_format tests
    // ========================================================================

    #[test]
    fn test_convert_hermes_stdio_to_unified() {
        let spec = json!({
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-filesystem"],
            "env": { "HOME": "/Users/test" },
            "enabled": true,
            "timeout": 30,
            "connect_timeout": 10,
            "tools": { "include": ["read_file"] },
            "sampling": { "enabled": true }
        });

        let result = convert_from_hermes_format("filesystem", &spec).unwrap();
        assert_eq!(result["type"], "stdio");
        assert_eq!(result["command"], "npx");
        assert_eq!(result["args"][0], "-y");
        assert_eq!(result["args"][1], "@modelcontextprotocol/server-filesystem");
        assert_eq!(result["env"]["HOME"], "/Users/test");
        // Hermes-specific fields should be stripped
        assert!(result.get("enabled").is_none());
        assert!(result.get("timeout").is_none());
        assert!(result.get("connect_timeout").is_none());
        assert!(result.get("tools").is_none());
        assert!(result.get("sampling").is_none());
    }

    #[test]
    fn test_convert_hermes_http_to_unified() {
        let spec = json!({
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer xxx" },
            "enabled": true,
            "timeout": 60
        });

        let result = convert_from_hermes_format("remote-server", &spec).unwrap();
        assert_eq!(result["type"], "sse");
        assert_eq!(result["url"], "https://example.com/mcp");
        assert_eq!(result["headers"]["Authorization"], "Bearer xxx");
        // Hermes-specific fields should be stripped
        assert!(result.get("enabled").is_none());
        assert!(result.get("timeout").is_none());
    }

    #[test]
    fn test_convert_hermes_no_command_no_url_fails() {
        let spec = json!({ "enabled": true, "timeout": 30 });
        assert!(convert_from_hermes_format("bad-server", &spec).is_err());
    }

    // ========================================================================
    // Merge-on-write tests
    // ========================================================================

    #[test]
    fn test_merge_preserves_hermes_specific_fields() {
        let existing = json!({
            "command": "old-cmd",
            "args": ["old-arg"],
            "enabled": true,
            "timeout": 30,
            "connect_timeout": 10,
            "tools": { "include": ["read_file"] },
            "sampling": { "enabled": true }
        });

        let new_spec = json!({
            "command": "new-cmd",
            "args": ["new-arg"],
            "env": { "KEY": "value" },
            "enabled": true
        });

        let merged = merge_hermes_spec(&existing, &new_spec);

        // Core fields should be overwritten
        assert_eq!(merged["command"], "new-cmd");
        assert_eq!(merged["args"][0], "new-arg");
        assert_eq!(merged["env"]["KEY"], "value");

        // Hermes-specific fields should be preserved from existing
        assert_eq!(merged["timeout"], 30);
        assert_eq!(merged["connect_timeout"], 10);
        assert_eq!(merged["tools"]["include"][0], "read_file");
        assert_eq!(merged["sampling"]["enabled"], true);
        assert_eq!(merged["enabled"], true);
    }

    #[test]
    fn test_merge_preserves_auth_field() {
        let existing = json!({
            "url": "https://mcp.example.com",
            "auth": "oauth",
            "enabled": true
        });

        let new_spec = json!({
            "url": "https://mcp.example.com/updated",
            "headers": { "X-Trace": "abc" },
            "enabled": true
        });

        let merged = merge_hermes_spec(&existing, &new_spec);

        assert_eq!(merged["url"], "https://mcp.example.com/updated");
        assert_eq!(merged["headers"]["X-Trace"], "abc");
        assert_eq!(
            merged["auth"], "oauth",
            "auth declaration must survive CC Switch round-trip"
        );
    }

    #[test]
    fn test_convert_hermes_strips_auth_on_import() {
        let spec = json!({
            "url": "https://mcp.example.com",
            "auth": "oauth",
            "enabled": true
        });

        let result = convert_from_hermes_format("remote", &spec).unwrap();
        assert_eq!(result["type"], "sse");
        assert_eq!(result["url"], "https://mcp.example.com");
        assert!(
            result.get("auth").is_none(),
            "auth stays Hermes-specific; stripped from unified format"
        );
    }

    #[test]
    fn test_merge_new_server_no_existing_extra_fields() {
        let existing = json!({
            "command": "old-cmd"
        });

        let new_spec = json!({
            "command": "new-cmd",
            "args": ["arg1"],
            "enabled": true
        });

        let merged = merge_hermes_spec(&existing, &new_spec);
        assert_eq!(merged["command"], "new-cmd");
        assert_eq!(merged["args"][0], "arg1");
        assert_eq!(merged["enabled"], true);
        // No extra fields to preserve
        assert!(merged.get("timeout").is_none());
    }
}
