//! MCP command handlers

use crate::cli::McpCommands;
use crate::output::Printer;
use cc_switch_core::AppState;

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
    todo!("Implement MCP add")
}

async fn handle_edit(
    id: &str,
    enable_app: Option<&str>,
    disable_app: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement MCP edit")
}

async fn handle_delete(
    id: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement MCP delete")
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
    println!("✓ {} MCP server '{}' for {}", action, id, app);
    Ok(())
}

async fn handle_import(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let count = cc_switch_core::McpService::import_from_claude(state)?;
    println!("✓ Imported {} MCP servers from Claude", count);
    Ok(())
}

fn parse_app_type(s: &str) -> anyhow::Result<cc_switch_core::AppType> {
    s.parse().map_err(|_| {
        anyhow::anyhow!(
            "Invalid app type: {}. Valid values: claude, codex, gemini, opencode",
            s
        )
    })
}
