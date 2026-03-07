//! MCP command handlers

use crate::cli::McpCommands;
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use anyhow::Context;
use cc_switch_core::{AppState, AppType, McpApps, McpServer};
use serde_json::{json, Value};
use std::fs;

pub async fn handle(cmd: McpCommands, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        McpCommands::List => handle_list(state, printer).await,
        McpCommands::Show { id } => handle_show(&id, state, printer).await,
        McpCommands::Add {
            id,
            command,
            args,
            apps,
            from_json,
        } => {
            handle_add(
                id.as_deref(),
                command.as_deref(),
                args.as_deref(),
                apps.as_deref(),
                from_json.as_deref(),
                state,
                printer,
            )
            .await
        }
        McpCommands::Edit {
            id,
            enable_app,
            disable_app,
        } => {
            handle_edit(
                &id,
                enable_app.as_deref(),
                disable_app.as_deref(),
                state,
                printer,
            )
            .await
        }
        McpCommands::Delete { id, yes } => handle_delete(&id, yes, state, printer).await,
        McpCommands::Enable { id, app } => handle_toggle(&id, &app, true, state, printer).await,
        McpCommands::Disable { id, app } => handle_toggle(&id, &app, false, state, printer).await,
        McpCommands::Import => handle_import(state, printer).await,
    }
}

async fn handle_list(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let servers = cc_switch_core::McpService::get_all_servers(state)?;
    printer.print_mcp_servers(&servers)?;
    Ok(())
}

async fn handle_show(id: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let servers = cc_switch_core::McpService::get_all_servers(state)?;
    let server = servers
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("MCP server not found: {}", id))?;
    printer.print_mcp_server_detail(server)?;
    Ok(())
}

async fn handle_add(
    id: Option<&str>,
    command: Option<&str>,
    args: Option<&str>,
    apps: Option<&str>,
    from_json: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let server = if let Some(path) = from_json {
        load_mcp_server_from_file(path, id, apps)?
    } else {
        let id = id.ok_or_else(|| anyhow::anyhow!("MCP add requires --id"))?;
        let command = command.ok_or_else(|| anyhow::anyhow!("MCP add requires --command"))?;
        let parsed_args = parse_cli_args(args);
        McpServer {
            id: id.to_string(),
            name: id.to_string(),
            server: json!({
                "type": "stdio",
                "command": command,
                "args": parsed_args,
            }),
            apps: parse_mcp_apps(apps)?,
            description: None,
            homepage: None,
            docs: None,
            tags: vec![],
        }
    };

    cc_switch_core::McpService::upsert_server(state, server.clone())?;
    printer.success(format!("✓ Added MCP server '{}'", server.id));
    Ok(())
}

async fn handle_edit(
    id: &str,
    enable_app: Option<&str>,
    disable_app: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    if enable_app.is_none() && disable_app.is_none() {
        anyhow::bail!("MCP edit requires --enable-app or --disable-app");
    }

    for app in parse_app_list(enable_app)? {
        cc_switch_core::McpService::toggle_app(state, id, app.clone(), true)?;
    }
    for app in parse_app_list(disable_app)? {
        cc_switch_core::McpService::toggle_app(state, id, app.clone(), false)?;
    }

    printer.success(format!("✓ Updated MCP server '{}'", id));
    Ok(())
}

async fn handle_delete(
    id: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    if !yes {
        anyhow::bail!("MCP delete is destructive. Re-run with --yes to confirm.");
    }

    let deleted = cc_switch_core::McpService::delete_server(state, id)?;
    if !deleted {
        anyhow::bail!("MCP server not found: {}", id);
    }

    printer.success(format!("✓ Deleted MCP server '{}'", id));
    Ok(())
}

async fn handle_toggle(
    id: &str,
    app: &str,
    enabled: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    cc_switch_core::McpService::toggle_app(state, id, app_type, enabled)?;
    let action = if enabled { "enabled" } else { "disabled" };
    printer.success(format!("✓ {} MCP server '{}' for {}", action, id, app));
    Ok(())
}

async fn handle_import(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let count = cc_switch_core::McpService::import_from_claude(state)?
        + cc_switch_core::McpService::import_from_codex(state)?
        + cc_switch_core::McpService::import_from_gemini(state)?
        + cc_switch_core::McpService::import_from_opencode(state)?;
    printer.success(format!(
        "✓ Imported {} MCP servers from live app configs",
        count
    ));
    Ok(())
}

fn parse_cli_args(args: Option<&str>) -> Vec<String> {
    args.map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(ToOwned::to_owned)
            .collect()
    })
    .unwrap_or_default()
}

fn parse_mcp_apps(apps: Option<&str>) -> anyhow::Result<McpApps> {
    let parsed = parse_app_list(apps)?;
    if parsed.is_empty() {
        return Ok(McpApps::only(&AppType::Claude));
    }

    let mut result = McpApps::default();
    for app in parsed {
        result.set_enabled_for(&app, true);
    }
    Ok(result)
}

fn parse_app_list(apps: Option<&str>) -> anyhow::Result<Vec<AppType>> {
    let Some(apps) = apps else {
        return Ok(vec![]);
    };

    apps.split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(parse_app_type)
        .collect()
}

fn load_mcp_server_from_file(
    path: &str,
    id_override: Option<&str>,
    apps_override: Option<&str>,
) -> anyhow::Result<McpServer> {
    let content = fs::read_to_string(path).with_context(|| format!("Failed to read file: {}", path))?;
    let value: Value =
        serde_json::from_str(&content).with_context(|| format!("Invalid JSON file: {}", path))?;

    let mut server = match serde_json::from_value::<McpServer>(value.clone()) {
        Ok(server) => server,
        Err(_) => {
            let id = id_override.ok_or_else(|| {
                anyhow::anyhow!("MCP JSON spec needs --id when the file does not contain a full server object")
            })?;
            McpServer {
                id: id.to_string(),
                name: id.to_string(),
                server: extract_server_spec(value)?,
                apps: McpApps::default(),
                description: None,
                homepage: None,
                docs: None,
                tags: vec![],
            }
        }
    };

    if let Some(id) = id_override {
        server.id = id.to_string();
        if server.name.trim().is_empty() {
            server.name = id.to_string();
        }
    }

    if let Some(apps) = apps_override {
        server.apps = parse_mcp_apps(Some(apps))?;
    } else if server.apps.is_empty() {
        server.apps = McpApps::only(&AppType::Claude);
    }

    Ok(server)
}

fn extract_server_spec(value: Value) -> anyhow::Result<Value> {
    if let Some(server) = value.get("server").cloned() {
        return Ok(server);
    }
    Ok(value)
}
