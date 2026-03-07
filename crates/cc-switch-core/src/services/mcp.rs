//! MCP service - business logic for MCP server management.

use indexmap::IndexMap;
use serde_json::Value;

use crate::app_config::{AppType, McpApps, McpServer};
use crate::error::AppError;
use crate::mcp::validation::validate_server_spec;
use crate::store::AppState;

pub struct McpService;

impl McpService {
    pub fn get_all_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
        state.db.get_all_mcp_servers()
    }

    pub fn get_server(state: &AppState, id: &str) -> Result<Option<McpServer>, AppError> {
        state.db.get_mcp_server(id)
    }

    pub fn upsert_server(state: &AppState, server: McpServer) -> Result<(), AppError> {
        validate_server_spec(&server.server)?;

        let prev_apps = state
            .db
            .get_all_mcp_servers()?
            .get(&server.id)
            .map(|item| item.apps.clone())
            .unwrap_or_default();

        state.db.save_mcp_server(&server)?;

        if prev_apps.claude && !server.apps.claude {
            Self::remove_server_from_app(&server.id, &AppType::Claude)?;
        }
        if prev_apps.codex && !server.apps.codex {
            Self::remove_server_from_app(&server.id, &AppType::Codex)?;
        }
        if prev_apps.gemini && !server.apps.gemini {
            Self::remove_server_from_app(&server.id, &AppType::Gemini)?;
        }
        if prev_apps.opencode && !server.apps.opencode {
            Self::remove_server_from_app(&server.id, &AppType::OpenCode)?;
        }

        Self::sync_server_to_apps(&server)?;
        Ok(())
    }

    pub fn save_server(state: &AppState, server: &McpServer) -> Result<(), AppError> {
        Self::upsert_server(state, server.clone())
    }

    pub fn delete_server(state: &AppState, id: &str) -> Result<bool, AppError> {
        let server = state.db.get_all_mcp_servers()?.shift_remove(id);
        let Some(server) = server else {
            return Ok(false);
        };

        state.db.delete_mcp_server(id)?;
        for app in server.apps.enabled_apps() {
            Self::remove_server_from_app(id, &app)?;
        }

        Ok(true)
    }

    pub fn toggle_app(
        state: &AppState,
        id: &str,
        app: AppType,
        enabled: bool,
    ) -> Result<(), AppError> {
        let mut server = state
            .db
            .get_mcp_server(id)?
            .ok_or_else(|| AppError::Message(format!("MCP server {id} not found")))?;

        server.apps.set_enabled_for(&app, enabled);
        state.db.save_mcp_server(&server)?;

        if enabled {
            Self::sync_server_to_app(&server, &app)?;
        } else {
            Self::remove_server_from_app(id, &app)?;
        }

        Ok(())
    }

    pub fn import_from_claude(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_map(
            state,
            crate::mcp::claude::read_mcp_servers_map()?,
            AppType::Claude,
        )
    }

    pub fn import_from_codex(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_map(
            state,
            crate::mcp::codex::read_mcp_servers_map()?,
            AppType::Codex,
        )
    }

    pub fn import_from_gemini(state: &AppState) -> Result<usize, AppError> {
        Self::import_from_map(
            state,
            crate::mcp::gemini::read_mcp_servers_map()?,
            AppType::Gemini,
        )
    }

    pub fn import_from_opencode(state: &AppState) -> Result<usize, AppError> {
        let raw = crate::mcp::opencode::read_mcp_servers_map()?;
        let mut converted = std::collections::HashMap::new();

        for (id, spec) in raw {
            let unified = crate::mcp::opencode::convert_from_opencode_format(&spec)?;
            validate_server_spec(&unified)?;
            converted.insert(id, unified);
        }

        Self::import_from_map(state, converted, AppType::OpenCode)
    }

    pub fn sync_all_enabled(state: &AppState) -> Result<(), AppError> {
        for server in Self::get_all_servers(state)?.values() {
            Self::sync_server_to_apps(server)?;
        }
        Ok(())
    }

    fn import_from_map(
        state: &AppState,
        servers: std::collections::HashMap<String, Value>,
        app: AppType,
    ) -> Result<usize, AppError> {
        let mut existing = state.db.get_all_mcp_servers()?;
        let mut new_count = 0usize;

        for (id, spec) in servers {
            validate_server_spec(&spec)?;

            let to_save = if let Some(existing_server) = existing.get(&id) {
                let mut merged = existing_server.clone();
                merged.apps.set_enabled_for(&app, true);
                merged
            } else {
                new_count += 1;
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec,
                    apps: McpApps::only(&app),
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: vec![],
                }
            };

            state.db.save_mcp_server(&to_save)?;
            Self::sync_server_to_apps(&to_save)?;
            existing.insert(to_save.id.clone(), to_save);
        }

        Ok(new_count)
    }

    fn sync_server_to_apps(server: &McpServer) -> Result<(), AppError> {
        for app in server.apps.enabled_apps() {
            Self::sync_server_to_app(server, &app)?;
        }
        Ok(())
    }

    fn sync_server_to_app(server: &McpServer, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => {
                crate::mcp::claude::sync_single_server_to_claude(&server.id, &server.server)
            }
            AppType::Codex => {
                crate::mcp::codex::sync_single_server_to_codex(&server.id, &server.server)
            }
            AppType::Gemini => {
                crate::mcp::gemini::sync_single_server_to_gemini(&server.id, &server.server)
            }
            AppType::OpenCode => {
                crate::mcp::opencode::sync_single_server_to_opencode(&server.id, &server.server)
            }
            AppType::OpenClaw => Ok(()),
        }
    }

    fn remove_server_from_app(id: &str, app: &AppType) -> Result<(), AppError> {
        match app {
            AppType::Claude => crate::mcp::claude::remove_server_from_claude(id),
            AppType::Codex => crate::mcp::codex::remove_server_from_codex(id),
            AppType::Gemini => crate::mcp::gemini::remove_server_from_gemini(id),
            AppType::OpenCode => crate::mcp::opencode::remove_server_from_opencode(id),
            AppType::OpenClaw => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use serial_test::serial;
    use tempfile::tempdir;

    use super::*;
    use crate::database::Database;

    #[test]
    #[serial]
    fn sync_all_enabled_writes_claude_and_opencode_live_config() -> Result<(), AppError> {
        let temp = tempdir().expect("tempdir");
        std::env::set_var("CC_SWITCH_TEST_HOME", temp.path());
        std::fs::create_dir_all(temp.path().join(".claude")).expect("claude dir");
        std::fs::create_dir_all(temp.path().join(".config/opencode")).expect("opencode dir");

        let state = AppState::new(Database::memory()?);
        McpService::upsert_server(
            &state,
            McpServer {
                id: "demo".to_string(),
                name: "demo".to_string(),
                server: json!({
                    "type": "stdio",
                    "command": "npx",
                    "args": ["@modelcontextprotocol/server-memory"]
                }),
                apps: McpApps {
                    claude: true,
                    codex: false,
                    gemini: false,
                    opencode: true,
                    openclaw: false,
                },
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            },
        )?;

        let claude_text =
            std::fs::read_to_string(crate::config::get_claude_mcp_path()).expect("claude mcp");
        assert!(claude_text.contains("\"demo\""));

        let opencode = crate::opencode_config::read_opencode_config()?;
        assert!(opencode.get("mcp").and_then(|v| v.get("demo")).is_some());

        Ok(())
    }
}
