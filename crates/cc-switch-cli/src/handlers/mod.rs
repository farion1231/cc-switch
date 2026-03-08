//! CLI Command Handlers

mod backup;
mod common;
mod config;
mod env;
mod import_export;
mod mcp;
mod prompt;
mod provider;
mod proxy;
mod skill;
mod usage;
mod workspace;

use crate::cli::{Cli, Commands, DeeplinkCommands};
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
        Commands::Backup { cmd } => backup::handle(cmd, &state, &printer).await,
        Commands::Env { cmd } => env::handle(cmd, &printer).await,
        Commands::Workspace { cmd } => workspace::handle(cmd, &printer).await,
        Commands::Deeplink { cmd } => match cmd {
            DeeplinkCommands::Parse { url } => {
                import_export::handle_deeplink_parse(&url, &printer).await
            }
            DeeplinkCommands::Merge { url } => {
                import_export::handle_deeplink_merge(&url, &printer).await
            }
            DeeplinkCommands::Preview { url } => {
                import_export::handle_deeplink_preview(&url, &printer).await
            }
        },
        Commands::Export { output } => {
            import_export::handle_export(&output, &state, &printer).await
        }
        Commands::Import { input, merge } => {
            import_export::handle_import(&input, merge, &state, &printer).await
        }
        Commands::ImportDeeplink { url } => {
            import_export::handle_deeplink(&url, &state, &printer).await
        }
        Commands::E2eSession => Ok(()),
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
        Commands::Backup { .. } => "backup",
        Commands::Env { .. } => "env",
        Commands::Workspace { .. } => "workspace",
        Commands::Deeplink { .. } => "deeplink",
        Commands::Export { .. } => "export",
        Commands::Import { .. } => "import",
        Commands::ImportDeeplink { .. } => "import-deeplink",
        Commands::E2eSession => "e2e-session",
    }
}
