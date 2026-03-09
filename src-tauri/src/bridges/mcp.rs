use std::collections::HashMap;

use indexmap::IndexMap;
use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::app_config::{AppType, McpServer};
use crate::claude_mcp;
use crate::error::AppError;
use crate::services::McpService as LegacyMcpService;
use crate::store::AppState;

fn map_core_err(err: cc_switch_core::AppError) -> AppError {
    AppError::Message(err.to_string())
}

fn convert<T, U>(value: T) -> Result<U, AppError>
where
    T: Serialize,
    U: DeserializeOwned,
{
    let value = serde_json::to_value(value).map_err(|e| AppError::JsonSerialize { source: e })?;
    serde_json::from_value(value).map_err(|e| AppError::Config(e.to_string()))
}

fn to_core_app_type(app_type: AppType) -> cc_switch_core::AppType {
    match app_type {
        AppType::Claude => cc_switch_core::AppType::Claude,
        AppType::Codex => cc_switch_core::AppType::Codex,
        AppType::Gemini => cc_switch_core::AppType::Gemini,
        AppType::OpenCode => cc_switch_core::AppType::OpenCode,
        AppType::OpenClaw => cc_switch_core::AppType::OpenClaw,
    }
}

fn core_state() -> Result<cc_switch_core::AppState, AppError> {
    let state = cc_switch_core::AppState::new(
        cc_switch_core::Database::new().map_err(map_core_err)?,
    );
    state.run_startup_maintenance();
    Ok(state)
}

fn with_core_state<T>(
    f: impl FnOnce(&cc_switch_core::AppState) -> Result<T, cc_switch_core::AppError>,
) -> Result<T, AppError> {
    let state = core_state()?;
    f(&state).map_err(map_core_err)
}

pub fn get_claude_mcp_status() -> Result<claude_mcp::McpStatus, AppError> {
    claude_mcp::get_mcp_status()
}

pub fn read_claude_mcp_config() -> Result<Option<String>, AppError> {
    claude_mcp::read_mcp_json()
}

pub fn upsert_claude_mcp_server(id: &str, spec: serde_json::Value) -> Result<bool, AppError> {
    claude_mcp::upsert_mcp_server(id, spec)
}

pub fn delete_claude_mcp_server(id: &str) -> Result<bool, AppError> {
    claude_mcp::delete_mcp_server(id)
}

pub fn validate_mcp_command(cmd: &str) -> Result<bool, AppError> {
    claude_mcp::validate_command_in_path(cmd)
}

pub fn legacy_get_mcp_servers_for_app(
    state: &AppState,
    app: AppType,
) -> Result<HashMap<String, serde_json::Value>, AppError> {
    #[allow(deprecated)]
    {
        LegacyMcpService::get_servers(state, app)
    }
}

pub fn get_mcp_servers_for_app(app: AppType) -> Result<HashMap<String, serde_json::Value>, AppError> {
    let all_servers: IndexMap<String, cc_switch_core::McpServer> =
        with_core_state(cc_switch_core::McpService::get_all_servers)?;

    let mut result = HashMap::new();
    for (id, server) in all_servers {
        if server.apps.is_enabled_for(&to_core_app_type(app.clone())) {
            result.insert(id, server.server);
        }
    }
    Ok(result)
}

pub fn legacy_upsert_mcp_server_in_config(
    state: &AppState,
    app: AppType,
    id: &str,
    spec: serde_json::Value,
    sync_other_side: Option<bool>,
) -> Result<bool, AppError> {
    let existing_server = state.db.get_all_mcp_servers()?.get(id).cloned();
    let mut new_server = if let Some(mut existing) = existing_server {
        existing.server = spec.clone();
        existing.apps.set_enabled_for(&app, true);
        existing
    } else {
        let mut apps = crate::app_config::McpApps::default();
        apps.set_enabled_for(&app, true);
        let name = spec
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or(id)
            .to_string();

        McpServer {
            id: id.to_string(),
            name,
            server: spec,
            apps,
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        }
    };

    if sync_other_side.unwrap_or(false) {
        new_server.apps.claude = true;
        new_server.apps.codex = true;
        new_server.apps.gemini = true;
        new_server.apps.opencode = true;
    }

    LegacyMcpService::upsert_server(state, new_server)?;
    Ok(true)
}

pub fn upsert_mcp_server_in_config(
    app: AppType,
    id: &str,
    spec: serde_json::Value,
    sync_other_side: Option<bool>,
) -> Result<bool, AppError> {
    with_core_state(|state| {
        let existing_server = state.db.get_all_mcp_servers()?.get(id).cloned();
        let mut new_server = if let Some(mut existing) = existing_server {
            existing.server = spec.clone();
            existing
        } else {
            let mut apps = cc_switch_core::McpApps::default();
            apps.set_enabled_for(&to_core_app_type(app.clone()), true);
            let name = spec
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or(id)
                .to_string();

            cc_switch_core::McpServer {
                id: id.to_string(),
                name,
                server: spec,
                apps,
                description: None,
                homepage: None,
                docs: None,
                tags: Vec::new(),
            }
        };

        new_server
            .apps
            .set_enabled_for(&to_core_app_type(app.clone()), true);

        if sync_other_side.unwrap_or(false) {
            new_server.apps.claude = true;
            new_server.apps.codex = true;
            new_server.apps.gemini = true;
            new_server.apps.opencode = true;
        }

        cc_switch_core::McpService::upsert_server(state, new_server)?;
        Ok(true)
    })
}

pub fn legacy_delete_mcp_server_in_config(state: &AppState, id: &str) -> Result<bool, AppError> {
    LegacyMcpService::delete_server(state, id)
}

pub fn delete_mcp_server_in_config(id: &str) -> Result<bool, AppError> {
    with_core_state(|state| cc_switch_core::McpService::delete_server(state, id))
}

pub fn legacy_set_mcp_enabled(
    state: &AppState,
    app: AppType,
    id: &str,
    enabled: bool,
) -> Result<bool, AppError> {
    #[allow(deprecated)]
    {
        LegacyMcpService::set_enabled(state, app, id, enabled)
    }
}

pub fn set_mcp_enabled(app: AppType, id: &str, enabled: bool) -> Result<bool, AppError> {
    with_core_state(|state| {
        cc_switch_core::McpService::toggle_app(state, id, to_core_app_type(app), enabled)?;
        Ok(true)
    })
}

pub fn legacy_get_all_mcp_servers(state: &AppState) -> Result<IndexMap<String, McpServer>, AppError> {
    LegacyMcpService::get_all_servers(state)
}

pub fn get_all_mcp_servers() -> Result<IndexMap<String, McpServer>, AppError> {
    let servers = with_core_state(cc_switch_core::McpService::get_all_servers)?;
    convert(servers)
}

pub fn legacy_upsert_mcp_server(state: &AppState, server: McpServer) -> Result<(), AppError> {
    LegacyMcpService::upsert_server(state, server)
}

pub fn upsert_mcp_server(server: McpServer) -> Result<(), AppError> {
    let server = convert(server)?;
    with_core_state(|state| cc_switch_core::McpService::upsert_server(state, server))
}

pub fn legacy_delete_mcp_server(state: &AppState, id: &str) -> Result<bool, AppError> {
    LegacyMcpService::delete_server(state, id)
}

pub fn delete_mcp_server(id: &str) -> Result<bool, AppError> {
    with_core_state(|state| cc_switch_core::McpService::delete_server(state, id))
}

pub fn legacy_toggle_mcp_app(
    state: &AppState,
    server_id: &str,
    app: AppType,
    enabled: bool,
) -> Result<(), AppError> {
    LegacyMcpService::toggle_app(state, server_id, app, enabled)
}

pub fn toggle_mcp_app(server_id: &str, app: AppType, enabled: bool) -> Result<(), AppError> {
    with_core_state(|state| {
        cc_switch_core::McpService::toggle_app(state, server_id, to_core_app_type(app), enabled)
    })
}

pub fn legacy_import_mcp_from_apps(state: &AppState) -> Result<usize, AppError> {
    let mut total = 0;
    total += LegacyMcpService::import_from_claude(state).unwrap_or(0);
    total += LegacyMcpService::import_from_codex(state).unwrap_or(0);
    total += LegacyMcpService::import_from_gemini(state).unwrap_or(0);
    total += LegacyMcpService::import_from_opencode(state).unwrap_or(0);
    Ok(total)
}

pub fn import_mcp_from_apps() -> Result<usize, AppError> {
    with_core_state(|state| {
        let mut total = 0;
        total += cc_switch_core::McpService::import_from_claude(state)?;
        total += cc_switch_core::McpService::import_from_codex(state)?;
        total += cc_switch_core::McpService::import_from_gemini(state)?;
        total += cc_switch_core::McpService::import_from_opencode(state)?;
        Ok(total)
    })
}
