//! CLI command implementations

use console::{style, Term};
use std::process::ExitCode;
use std::sync::Arc;

use super::args::{parse_tool_name, CmdAction};
use super::interactive;
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
            // Interactive mode
            match interactive::run_interactive(&app_state, &term) {
                Ok(_) => ExitCode::SUCCESS,
                Err(e) => {
                    eprintln!("{} {e}", style("✗").red());
                    ExitCode::from(1)
                }
            }
        }
        Some(CmdAction::Status) => cmd_status(&app_state, &term),
        Some(CmdAction::List { tool }) => cmd_list(&app_state, &term, &tool),
        Some(CmdAction::Switch { tool, provider }) => {
            cmd_switch(&app_state, &term, &tool, &provider)
        }
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
fn cmd_list(state: &AppState, term: &Term, tool: &str) -> ExitCode {
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

        let _ = term.write_line(&format!("  {marker} {name}{active_label}"));
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

    // Find provider by name (case-insensitive partial match)
    let matching: Vec<_> = providers
        .iter()
        .filter(|(_, p)| p.name.to_lowercase().contains(&provider_name.to_lowercase()))
        .collect();

    let (provider_id, provider) = match matching.len() {
        0 => {
            eprintln!(
                "{} Provider '{}' not found for {}.",
                style("✗").red(),
                provider_name,
                app_type.as_str()
            );
            eprintln!("\nAvailable providers:");
            for (_, p) in providers.iter() {
                eprintln!("  • {}", p.name);
            }
            return ExitCode::from(1);
        }
        1 => matching[0],
        _ => {
            // Check for exact match first
            if let Some(exact) = matching
                .iter()
                .find(|(_, p)| p.name.to_lowercase() == provider_name.to_lowercase())
            {
                *exact
            } else {
                eprintln!(
                    "{} Multiple providers match '{}'. Please be more specific:",
                    style("✗").red(),
                    provider_name
                );
                for (_, p) in matching {
                    eprintln!("  • {}", p.name);
                }
                return ExitCode::from(1);
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
        "    {}        Enter interactive mode (default)",
        style("(none)").dim()
    ));
    let _ = term.write_line(&format!(
        "    {}        Show current active provider for all tools",
        style("status").green()
    ));
    let _ = term.write_line(&format!(
        "    {} {}   List all providers for a tool",
        style("list").green(),
        style("<tool>").yellow()
    ));
    let _ = term.write_line(&format!(
        "    {} {} {}",
        style("switch").green(),
        style("<tool>").yellow(),
        style("<provider>").yellow()
    ));
    let _ = term.write_line("                        Switch to a specific provider");
    let _ = term.write_line(&format!(
        "    {}          Show this help message",
        style("help").green()
    ));
    let _ = term.write_line("");

    let _ = term.write_line(&format!("{}", style("TOOLS:").bold().underlined()));
    let _ = term.write_line("    claude, codex, gemini");
    let _ = term.write_line("");

    let _ = term.write_line(&format!("{}", style("EXAMPLES:").bold().underlined()));
    let _ = term.write_line(&format!(
        "    {}",
        style("# Enter interactive mode").dim()
    ));
    let _ = term.write_line("    cc-switch cmd");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# Show all active providers").dim()
    ));
    let _ = term.write_line("    cc-switch cmd status");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# List Claude providers").dim()
    ));
    let _ = term.write_line("    cc-switch cmd list claude");
    let _ = term.write_line("");
    let _ = term.write_line(&format!(
        "    {}",
        style("# Switch Claude provider").dim()
    ));
    let _ = term.write_line("    cc-switch cmd switch claude \"My Provider\"");
    let _ = term.write_line("");

    ExitCode::SUCCESS
}
