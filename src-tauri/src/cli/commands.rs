//! CLI command implementations

use console::{style, Term};
use std::process::ExitCode;
use std::sync::Arc;

use super::args::{parse_tool_name, CmdAction};
use super::crud;
use super::tui;
use crate::app_config::AppType;
use crate::database::Database;
use crate::services::ProviderService;
use crate::store::AppState;

/// Run the CLI mode
///
/// Initializes database connection directly (without Tauri runtime)
/// and executes the requested CLI action.
pub fn run_cli(action: Option<CmdAction>) -> ExitCode {
    let term = Term::stdout();

    // Initialize database
    let db = match Database::init() {
        Ok(db) => Arc::new(db),
        Err(e) => {
            eprintln!("{} Failed to initialize database: {e}", style("✗").red());
            return ExitCode::from(1);
        }
    };

    let app_state = AppState::new(db);

    match action {
        None => {
            // Interactive mode (TUI by default)
            match tui::run_tui(&app_state) {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{} {e}", style("✗").red());
                    ExitCode::from(1)
                }
            }
        }
        Some(CmdAction::Status) => cmd_status(&app_state, &term),
        Some(CmdAction::List { tool, ids }) => cmd_list(&app_state, &term, &tool, ids),
        Some(CmdAction::Switch { tool, provider }) => {
            cmd_switch(&app_state, &term, &tool, &provider)
        }
        Some(CmdAction::Add { tool, json }) => cmd_add(&app_state, &term, &tool, json.as_deref()),
        Some(CmdAction::Edit { tool, provider }) => cmd_edit(&app_state, &term, &tool, &provider),
        Some(CmdAction::Delete {
            tool,
            provider,
            force,
        }) => cmd_delete(&app_state, &term, &tool, &provider, force),
        Some(CmdAction::Show {
            tool,
            provider,
            json,
        }) => cmd_show(&app_state, &term, &tool, &provider, json),
        Some(CmdAction::Help) => cmd_help(&term),
    }
}

/// Display current active provider status for all tools
fn cmd_status(state: &AppState, term: &Term) -> ExitCode {
    let _ = term.write_line(&format!(
        "\n{}",
        style("Provider Status").bold().underlined()
    ));
    let _ = term.write_line("");

    let tools = [AppType::Claude, AppType::Codex, AppType::Gemini];

    for app in tools {
        let current_id = ProviderService::current(state, app.clone()).ok();
        let providers = ProviderService::list(state, app.clone()).ok();

        let status = match (&current_id, &providers) {
            (Some(id), Some(providers)) => {
                if let Some(provider) = providers.get(id) {
                    format!("{} {}", style("●").green(), provider.name)
                } else {
                    format!("{}", style("Not configured").dim())
                }
            }
            _ => format!("{}", style("Not configured").dim()),
        };

        let _ = term.write_line(&format!(
            "  {:8} {}",
            style(format!("{}:", app.as_str())).bold(),
            status
        ));
    }

    let _ = term.write_line("");
    ExitCode::SUCCESS
}

/// List all providers for a specific tool
fn cmd_list(state: &AppState, term: &Term, tool: &str, show_ids: bool) -> ExitCode {
    let app_type = match parse_tool_name(tool) {
        Some(t) => t,
        None => {
            eprintln!(
                "{} Invalid tool name: '{}'. Valid options: claude, codex, gemini",
                style("✗").red(),
                tool
            );
            return ExitCode::from(1);
        }
    };

    let providers = match ProviderService::list(state, app_type.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Failed to list providers: {e}", style("✗").red());
            return ExitCode::from(1);
        }
    };

    if providers.is_empty() {
        let _ = term.write_line(&format!(
            "\n{} No providers configured for {}. Use the GUI to add providers.",
            style("ℹ").blue(),
            app_type.as_str()
        ));
        return ExitCode::SUCCESS;
    }

    let current_id = ProviderService::current(state, app_type.clone()).ok();

    let _ = term.write_line(&format!(
        "\n{}",
        style(format!("Providers for {}", app_type.as_str()))
            .bold()
            .underlined()
    ));
    let _ = term.write_line("");

    for (id, provider) in providers.iter() {
        let is_active = current_id.as_ref() == Some(id);

        let marker = if is_active {
            style("●").green().to_string()
        } else {
            style("○").dim().to_string()
        };

        let name = if is_active {
            style(&provider.name).green().bold().to_string()
        } else {
            provider.name.clone()
        };

        let active_label = if is_active {
            format!(" {}", style("[Active]").green())
        } else {
            String::new()
        };

        if show_ids {
            let _ = term.write_line(&format!("  {marker} {name} (id: {id}){active_label}"));
        } else {
            let _ = term.write_line(&format!("  {marker} {name}{active_label}"));
        }
    }

    let _ = term.write_line("");
    ExitCode::SUCCESS
}

/// Switch provider for a specific tool (non-interactive)
fn cmd_switch(state: &AppState, term: &Term, tool: &str, provider_name: &str) -> ExitCode {
    let app_type = match parse_tool_name(tool) {
        Some(t) => t,
        None => {
            eprintln!(
                "{} Invalid tool name: '{}'. Valid options: claude, codex, gemini",
                style("✗").red(),
                tool
            );
            return ExitCode::from(1);
        }
    };

    let providers = match ProviderService::list(state, app_type.clone()) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{} Failed to list providers: {e}", style("✗").red());
            return ExitCode::from(1);
        }
    };

    // Prefer switching by provider ID when an exact ID is provided.
    let (provider_id, provider) = if let Some(provider) = providers.get(provider_name) {
        (provider_name, provider)
    } else {
        // Find provider by name (case-insensitive partial match)
        let query = provider_name.to_ascii_lowercase();
        let matching: Vec<_> = providers
            .iter()
            .filter(|(_, p)| p.name.to_ascii_lowercase().contains(&query))
            .collect();

        match matching.len() {
            0 => {
                eprintln!(
                    "{} Provider '{}' not found for {}.",
                    style("✗").red(),
                    provider_name,
                    app_type.as_str()
                );
                if !providers.is_empty() {
                    eprintln!("\nAvailable providers (use ID to disambiguate):");
                    for (id, p) in providers.iter() {
                        eprintln!("  • {} (id: {id})", p.name);
                    }
                }
                return ExitCode::from(1);
            }
            1 => (matching[0].0.as_str(), matching[0].1),
            _ => {
                // If there is exactly one exact match, use it; otherwise require ID.
                let exact: Vec<_> = matching
                    .iter()
                    .filter(|(_, p)| p.name.eq_ignore_ascii_case(provider_name))
                    .collect();

                if exact.len() == 1 {
                    (exact[0].0.as_str(), exact[0].1)
                } else {
                    eprintln!(
                        "{} Multiple providers match '{}'. Please use a provider ID.",
                        style("✗").red(),
                        provider_name
                    );
                    eprintln!("Tip: run `cc-switch cmd list {tool} --ids` to see IDs.\n");
                    for (id, p) in matching {
                        eprintln!("  • {} (id: {id})", p.name);
                    }
                    return ExitCode::from(1);
                }
            }
        }
    };

    // Perform the switch
    match ProviderService::switch(state, app_type.clone(), provider_id) {
        Ok(_) => {
            let _ = term.write_line(&format!(
                "\n{} Switched {} provider to \"{}\"",
                style("✓").green(),
                app_type.as_str(),
                style(&provider.name).bold()
            ));
            let _ = term.write_line("");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{} Failed to switch provider: {e}", style("✗").red());
            ExitCode::from(1)
        }
    }
}

fn cmd_add(state: &AppState, term: &Term, tool: &str, json_path: Option<&str>) -> ExitCode {
    let app_type = match parse_tool_name(tool) {
        Some(t) => t,
        None => {
            eprintln!(
                "{} Invalid tool name: '{}'. Valid options: claude, codex, gemini",
                style("✗").red(),
                tool
            );
            return ExitCode::from(1);
        }
    };

    match crud::add_provider(state, term, app_type, json_path) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style("✗").red());
            ExitCode::from(1)
        }
    }
}

fn cmd_edit(state: &AppState, term: &Term, tool: &str, provider_name: &str) -> ExitCode {
    let app_type = match parse_tool_name(tool) {
        Some(t) => t,
        None => {
            eprintln!(
                "{} Invalid tool name: '{}'. Valid options: claude, codex, gemini",
                style("✗").red(),
                tool
            );
            return ExitCode::from(1);
        }
    };

    match crud::edit_provider(state, term, app_type, provider_name) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style("✗").red());
            ExitCode::from(1)
        }
    }
}

fn cmd_delete(
    state: &AppState,
    term: &Term,
    tool: &str,
    provider_name: &str,
    force: bool,
) -> ExitCode {
    let app_type = match parse_tool_name(tool) {
        Some(t) => t,
        None => {
            eprintln!(
                "{} Invalid tool name: '{}'. Valid options: claude, codex, gemini",
                style("✗").red(),
                tool
            );
            return ExitCode::from(1);
        }
    };

    match crud::delete_provider(state, term, app_type, provider_name, force) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style("✗").red());
            ExitCode::from(1)
        }
    }
}

fn cmd_show(
    state: &AppState,
    term: &Term,
    tool: &str,
    provider_name: &str,
    as_json: bool,
) -> ExitCode {
    let app_type = match parse_tool_name(tool) {
        Some(t) => t,
        None => {
            eprintln!(
                "{} Invalid tool name: '{}'. Valid options: claude, codex, gemini",
                style("✗").red(),
                tool
            );
            return ExitCode::from(1);
        }
    };

    match crud::show_provider(state, term, app_type, provider_name, as_json) {
        Ok(_) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{} {e}", style("✗").red());
            ExitCode::from(1)
        }
    }
}

/// Display detailed help with examples
fn cmd_help(term: &Term) -> ExitCode {
    let _ = term.write_line(&format!(
        "\n{}",
        style("CC-Switch CLI - Provider Management").bold().cyan()
    ));
    let _ = term.write_line("");

    let _ = term.write_line(&format!("{}", style("USAGE:").bold().underlined()));
    let _ = term.write_line("    cc-switch cmd [COMMAND]");
    let _ = term.write_line("");

    let _ = term.write_line(&format!("{}", style("COMMANDS:").bold().underlined()));
    let _ = term.write_line(&format!(
        "    {}        Launch TUI (interactive mode, default)",
        style("(none)").dim()
    ));
    let _ =
        term.write_line("                        Tip: press '?' inside the TUI to view shortcuts");
    let _ = term.write_line(&format!(
        "    {}        Show current active provider for all tools",
        style("status").green()
    ));
    let _ = term.write_line(&format!(
        "    {} {}   List all providers for a tool",
        style("list").green(),
        style("<tool> [--ids]").yellow()
    ));
    let _ = term.write_line(&format!(
        "    {} {} {}",
        style("switch").green(),
        style("<tool>").yellow(),
        style("<provider|id>").yellow()
    ));
    let _ = term.write_line("                        Switch to a specific provider");
    let _ = term.write_line(&format!(
        "    {} {}   Add a new provider (interactive)",
        style("add").green(),
        style("<tool>").yellow()
    ));
    let _ = term.write_line(&format!(
        "    {} {} {}",
        style("edit").green(),
        style("<tool>").yellow(),
        style("<provider|id>").yellow()
    ));
    let _ = term.write_line("                        Edit an existing provider");
    let _ = term.write_line(&format!(
        "    {} {} {}",
        style("delete").green(),
        style("<tool>").yellow(),
        style("<provider|id>").yellow()
    ));
    let _ = term.write_line("                        Delete a provider");
    let _ = term.write_line(&format!(
        "    {} {} {}",
        style("show").green(),
        style("<tool>").yellow(),
        style("<provider|id>").yellow()
    ));
    let _ = term.write_line("                        Show provider details");
    let _ = term.write_line(&format!(
        "    {}          Show this help message",
        style("help").green()
    ));
    let _ = term.write_line("");

    let _ = term.write_line(&format!("{}", style("TOOLS:").bold().underlined()));
    let _ = term.write_line("    claude, codex, gemini");
    let _ = term.write_line("");

    let _ = term.write_line(&format!("{}", style("EXAMPLES:").bold().underlined()));
    let _ = term.write_line(&format!("    {}", style("# Enter interactive mode").dim()));
    let _ = term.write_line("    cc-switch cmd");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# Show all active providers").dim()
    ));
    let _ = term.write_line("    cc-switch cmd status");
    let _ = term.write_line("");
    let _ = term.write_line(&format!("    {}", style("# List Claude providers").dim()));
    let _ = term.write_line("    cc-switch cmd list claude");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# List providers with IDs (for duplicates)").dim()
    ));
    let _ = term.write_line("    cc-switch cmd list claude --ids");
    let _ = term.write_line("");
    let _ = term.write_line(&format!("    {}", style("# Switch Claude provider").dim()));
    let _ = term.write_line("    cc-switch cmd switch claude \"My Provider\"");
    let _ = term.write_line("");
    let _ = term.write_line(&format!("    {}", style("# Switch provider by ID").dim()));
    let _ = term.write_line("    cc-switch cmd switch claude <provider-id>");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# Add a provider (interactive)").dim()
    ));
    let _ = term.write_line("    cc-switch cmd add claude");
    let _ = term.write_line("");
    let _ = term.write_line(&format!("    {}", style("# Show provider details").dim()));
    let _ = term.write_line("    cc-switch cmd show claude \"My Provider\"");
    let _ = term.write_line("");

    let _ = term.write_line(&format!(
        "{}",
        style("JSON IMPORT FORMAT:").bold().underlined()
    ));
    let _ = term.write_line(&format!("    {}", style("# Import from file").dim()));
    let _ = term.write_line("    cc-switch cmd add claude --json provider.json");
    let _ = term.write_line(&format!("    {}", style("# Import from stdin").dim()));
    let _ = term.write_line("    cat provider.json | cc-switch cmd add claude --json -");
    let _ = term.write_line("");
    let _ = term.write_line(&format!("    {}", style("# Claude example:").dim()));
    let _ = term.write_line("    {");
    let _ = term.write_line("      \"name\": \"My Claude Provider\",");
    let _ = term.write_line("      \"settingsConfig\": {");
    let _ = term.write_line("        \"env\": {");
    let _ = term.write_line("          \"ANTHROPIC_AUTH_TOKEN\": \"sk-ant-...\",");
    let _ = term.write_line("          \"ANTHROPIC_BASE_URL\": \"https://api.anthropic.com\"");
    let _ = term.write_line("        }");
    let _ = term.write_line("      }");
    let _ = term.write_line("    }");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# Codex simple example (auto-converts to TOML):").dim()
    ));
    let _ = term.write_line("    {");
    let _ = term.write_line("      \"name\": \"My Codex Provider\",");
    let _ = term.write_line("      \"settingsConfig\": {");
    let _ = term.write_line("        \"auth\": { \"OPENAI_API_KEY\": \"sk-...\" },");
    let _ = term.write_line("        \"model\": \"gpt-4\",");
    let _ = term.write_line("        \"baseUrl\": \"https://api.openai.com/v1\"");
    let _ = term.write_line("      }");
    let _ = term.write_line("    }");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# Codex full example (config as JSON object, auto-converts to TOML):").dim()
    ));
    let _ = term.write_line("    {");
    let _ = term.write_line("      \"name\": \"My Codex Provider\",");
    let _ = term.write_line("      \"settingsConfig\": {");
    let _ = term.write_line("        \"auth\": { \"OPENAI_API_KEY\": \"sk-...\" },");
    let _ = term.write_line("        \"config\": {");
    let _ = term.write_line("          \"model\": \"gpt-5.2\",");
    let _ = term.write_line("          \"model_provider\": \"custom\",");
    let _ = term.write_line("          \"model_providers\": {");
    let _ = term.write_line(
        "            \"custom\": { \"base_url\": \"https://...\", \"wire_api\": \"responses\" }",
    );
    let _ = term.write_line("          },");
    let _ = term.write_line("          \"mcp_servers\": { ... }");
    let _ = term.write_line("        }");
    let _ = term.write_line("      }");
    let _ = term.write_line("    }");
    let _ = term.write_line("");
    let _ = term.write_line(&format!("    {}", style("# Gemini example:").dim()));
    let _ = term.write_line("    {");
    let _ = term.write_line("      \"name\": \"My Gemini Provider\",");
    let _ = term.write_line("      \"settingsConfig\": {");
    let _ = term.write_line("        \"env\": {");
    let _ = term.write_line("          \"GEMINI_API_KEY\": \"...\",");
    let _ = term.write_line(
        "          \"GOOGLE_GEMINI_BASE_URL\": \"https://generativelanguage.googleapis.com\"",
    );
    let _ = term.write_line("        }");
    let _ = term.write_line("      }");
    let _ = term.write_line("    }");
    let _ = term.write_line("");

    ExitCode::SUCCESS
}
