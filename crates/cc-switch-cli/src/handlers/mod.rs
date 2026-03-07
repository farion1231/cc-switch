//! CLI Command Handlers

mod common;
mod config;
mod import_export;
mod mcp;
mod prompt;
mod provider;
mod proxy;
mod skill;
mod usage;

use crate::cli::{Cli, Commands};
use crate::output::Printer;
use cc_switch_core::AppState;

pub async fn dispatch(cli: Cli, state: AppState) -> anyhow::Result<()> {
    let printer = Printer::new(cli.format, cli.quiet, cli.verbose);
    printer.verbose(format!(
        "Executing {} command (format: {:?})",
        command_name(&cli.command),
        cli.format
    ));

    match cli.command {
        Commands::Provider { cmd } => provider::handle(cmd, &state, &printer).await,
        Commands::Mcp { cmd } => mcp::handle(cmd, &state, &printer).await,
        Commands::Proxy { cmd } => proxy::handle(cmd, &state, &printer).await,
        Commands::Prompt { cmd } => prompt::handle(cmd, &state, &printer).await,
        Commands::Skill { cmd } => skill::handle(cmd, &state, &printer).await,
        Commands::Config { cmd } => config::handle(cmd, &state, &printer).await,
        Commands::Usage { cmd } => usage::handle(cmd, &state, &printer).await,
        Commands::Export { output } => {
            import_export::handle_export(&output, &state, &printer).await
        }
        Commands::Import { input, merge } => {
            import_export::handle_import(&input, merge, &state, &printer).await
        }
        Commands::ImportDeeplink { url } => {
            import_export::handle_deeplink(&url, &state, &printer).await
        }
    }
}

fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Provider { .. } => "provider",
        Commands::Mcp { .. } => "mcp",
        Commands::Proxy { .. } => "proxy",
        Commands::Prompt { .. } => "prompt",
        Commands::Skill { .. } => "skill",
        Commands::Config { .. } => "config",
        Commands::Usage { .. } => "usage",
        Commands::Export { .. } => "export",
        Commands::Import { .. } => "import",
        Commands::ImportDeeplink { .. } => "import-deeplink",
    }
}
