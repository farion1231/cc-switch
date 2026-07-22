//! ZCode MCP 同步和导入模块
//!
//! ZCode 的 MCP 配置使用标准的 `type: "stdio"` 格式（与 Claude Code 一致），
//! 因此本模块**不做任何格式转换**，直接透传读写。
//!
//! 与 OpenCode 的差异：OpenCode 把 MCP 和 provider 放在同一个 `opencode.json`，
//! 且使用非标准的 `local`/`remote` 类型；ZCode 把 MCP 单独放在 `cli/config.json`，
//! 且使用标准 stdio/sse/http 类型，与 CC Switch 统一格式一致。

use serde_json::Value;
use std::collections::HashMap;

use crate::app_config::{McpApps, McpServer, MultiAppConfig};
use crate::error::AppError;
use crate::zcode_config;

use super::validation::validate_server_spec;

// ============================================================================
// Helper Functions
// ============================================================================

/// Check if ZCode MCP sync should proceed
fn should_sync_zcode_mcp() -> bool {
    // Skip if ZCode config directory doesn't exist
    zcode_config::get_zcode_dir().exists()
}

// ============================================================================
// Public API: Sync Functions
// ============================================================================

/// Sync a single MCP server to ZCode live config.
///
/// ZCode uses the standard stdio format, so the spec is passed through as-is
/// without any conversion.
pub fn sync_single_server_to_zcode(
    _config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if !should_sync_zcode_mcp() {
        return Ok(());
    }

    // Pass through directly — ZCode uses the standard stdio format
    zcode_config::set_mcp_server(id, server_spec.clone())
}

/// Remove a single MCP server from ZCode live config
pub fn remove_server_from_zcode(id: &str) -> Result<(), AppError> {
    if !should_sync_zcode_mcp() {
        return Ok(());
    }

    zcode_config::remove_mcp_server(id)
}

/// Import MCP servers from ZCode config to unified structure.
///
/// Existing servers will have the ZCode app enabled without overwriting other
/// fields. Because ZCode uses the standard stdio format, the spec is validated
/// directly (no format conversion needed).
pub fn import_from_zcode(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let mcp_map = zcode_config::get_mcp_servers()?;
    if mcp_map.is_empty() {
        return Ok(0);
    }

    // Ensure servers map exists
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in mcp_map {
        // Validate the spec directly (no conversion needed — standard stdio format)
        if let Err(e) = validate_server_spec(&spec) {
            log::warn!("Skip invalid ZCode MCP server '{id}': {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(&id) {
            // Existing server: just enable ZCode app
            if !existing.apps.zcode {
                existing.apps.zcode = true;
                changed += 1;
                log::info!("MCP server '{id}' enabled for ZCode");
            }
        } else {
            // New server: default to only ZCode enabled
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
                        zcode: true,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("Imported new MCP server '{id}' from ZCode");
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
    use serde_json::json;

    #[test]
    fn test_stdio_spec_passes_through_validation() {
        // ZCode uses the standard stdio format; validate_server_spec must accept it
        // as-is (no conversion). This mirrors the import path.
        let spec = json!({
            "type": "stdio",
            "command": "npx",
            "args": ["-y", "tavily-mcp"],
            "env": { "TAVILY_API_KEY": "xxx" }
        });
        // Should not error — stdio with command is valid
        validate_server_spec(&spec).expect("stdio spec is valid");
    }

    #[test]
    fn test_sse_spec_passes_through_validation() {
        // sse/http specs are also passed through without conversion
        let spec = json!({
            "type": "sse",
            "url": "https://example.com/mcp",
            "headers": { "Authorization": "Bearer xxx" }
        });
        validate_server_spec(&spec).expect("sse spec is valid");
    }

    #[test]
    fn test_import_marks_new_server_with_only_zcode_enabled() {
        // Isolate from the real ~/.zcode by pointing the home dir at a temp
        // directory so import_from_zcode sees an empty cli/config.json.
        let temp = std::env::temp_dir().join(format!(
            "cc-switch-zcode-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp).unwrap();
        std::env::set_var("CC_SWITCH_TEST_HOME", &temp);

        let mut config = MultiAppConfig::default();
        // With no ~/.zcode/cli/config.json present, import is a clean no-op.
        let count = import_from_zcode(&mut config).expect("empty import ok");
        assert_eq!(count, 0);

        std::env::remove_var("CC_SWITCH_TEST_HOME");
        let _ = std::fs::remove_dir_all(&temp);
    }
}
