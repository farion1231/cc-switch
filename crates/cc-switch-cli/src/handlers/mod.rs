//! CLI Command Handlers

mod backup;
mod common;
mod config;
mod doctor;
mod env;
mod host;
mod import_export;
mod mcp;
mod omo;
mod openclaw;
mod prompt;
mod provider;
mod proxy;
mod session;
mod settings;
mod skill;
mod usage;
mod webdav;
mod workspace;

use crate::cli::{Cli, Commands, DeeplinkCommands, InstallCommands, UpdateCommands};
use crate::output::Printer;
use cc_switch_core::AppState;

pub async fn dispatch(cli: Cli, state: AppState) -> anyhow::Result<()> {
    let printer = build_printer(&cli);
    dispatch_with_printer(cli, Some(state), printer).await
}

pub async fn dispatch_stateless(cli: Cli) -> anyhow::Result<()> {
    let printer = build_printer(&cli);
    dispatch_with_printer(cli, None, printer).await
}

pub fn command_needs_state(command: &Commands) -> bool {
    matches!(
        command,
        Commands::Provider { .. }
            | Commands::Mcp { .. }
            | Commands::Proxy { .. }
            | Commands::Prompt { .. }
            | Commands::Skill { .. }
            | Commands::Settings { .. }
            | Commands::Doctor { .. }
            | Commands::Config { .. }
            | Commands::Usage { .. }
            | Commands::Backup { .. }
            | Commands::Omo { .. }
            | Commands::OmoSlim { .. }
            | Commands::Webdav { .. }
            | Commands::Export { .. }
            | Commands::Import { .. }
            | Commands::ImportDeeplink { .. }
    )
}

fn build_printer(cli: &Cli) -> Printer {
    let printer = Printer::new(cli.format, cli.quiet, cli.verbose);
    printer.verbose(format!(
        "Executing {} command (format: {:?})",
        command_name(&cli.command),
        cli.format
    ));
    printer
}

async fn dispatch_with_printer(
    cli: Cli,
    state: Option<AppState>,
    printer: Printer,
) -> anyhow::Result<()> {
    let require_state = || anyhow::anyhow!("internal error: command requires application state");

    match cli.command {
        Commands::Completions { shell } => host::handle_completions(shell, &printer).await,
        Commands::Install { cmd } => match cmd {
            InstallCommands::Guide { shell } => host::handle_install_guide(shell, &printer).await,
            InstallCommands::Completions { shell, dir } => {
                host::handle_install_completions(shell, dir.as_deref(), &printer).await
            }
        },
        Commands::Provider { cmd } => {
            provider::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Mcp { cmd } => {
            mcp::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Proxy { cmd } => {
            proxy::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Prompt { cmd } => {
            prompt::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Skill { cmd } => {
            skill::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Settings { cmd } => {
            settings::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::AutoLaunch { cmd } => host::handle_auto_launch(cmd, &printer).await,
        Commands::PortableMode => host::handle_portable_mode(&printer).await,
        Commands::ToolVersions {
            tools,
            latest,
            wsl_shell,
            wsl_shell_flag,
        } => host::handle_tool_versions(tools, latest, wsl_shell, wsl_shell_flag, &printer).await,
        Commands::About => host::handle_about(&printer).await,
        Commands::Doctor {
            apps,
            latest,
            check_updates,
        } => {
            doctor::handle(
                apps,
                latest,
                check_updates,
                state.as_ref().ok_or_else(require_state)?,
                &printer,
            )
            .await
        }
        Commands::Update { cmd } => match cmd {
            UpdateCommands::Check => host::handle_update_check(&printer).await,
            UpdateCommands::Guide => host::handle_update_guide(&printer).await,
        },
        Commands::ReleaseNotes { latest } => host::handle_release_notes(latest, &printer).await,
        Commands::Config { cmd } => {
            config::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Usage { cmd } => {
            usage::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Backup { cmd } => {
            backup::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Env { cmd } => env::handle(cmd, &printer).await,
        Commands::Openclaw { cmd } => openclaw::handle(cmd, &printer).await,
        Commands::Sessions { cmd } => session::handle(cmd, &printer).await,
        Commands::Omo { cmd } => {
            omo::handle_standard(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::OmoSlim { cmd } => {
            omo::handle_slim(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
        Commands::Workspace { cmd } => workspace::handle(cmd, &printer).await,
        Commands::Webdav { cmd } => {
            webdav::handle(cmd, state.as_ref().ok_or_else(require_state)?, &printer).await
        }
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
            import_export::handle_export(
                &output,
                state.as_ref().ok_or_else(require_state)?,
                &printer,
            )
            .await
        }
        Commands::Import { input, merge } => {
            import_export::handle_import(
                &input,
                merge,
                state.as_ref().ok_or_else(require_state)?,
                &printer,
            )
            .await
        }
        Commands::ImportDeeplink { url } => {
            import_export::handle_deeplink(
                &url,
                state.as_ref().ok_or_else(require_state)?,
                &printer,
            )
            .await
        }
        Commands::E2eSession => Ok(()),
    }
}

fn command_name(command: &Commands) -> &'static str {
    match command {
        Commands::Completions { .. } => "completions",
        Commands::Install { .. } => "install",
        Commands::Provider { .. } => "provider",
        Commands::Mcp { .. } => "mcp",
        Commands::Proxy { .. } => "proxy",
        Commands::Prompt { .. } => "prompt",
        Commands::Skill { .. } => "skill",
        Commands::Settings { .. } => "settings",
        Commands::AutoLaunch { .. } => "auto-launch",
        Commands::PortableMode => "portable-mode",
        Commands::ToolVersions { .. } => "tool-versions",
        Commands::About => "about",
        Commands::Doctor { .. } => "doctor",
        Commands::Update { .. } => "update",
        Commands::ReleaseNotes { .. } => "release-notes",
        Commands::Config { .. } => "config",
        Commands::Usage { .. } => "usage",
        Commands::Backup { .. } => "backup",
        Commands::Env { .. } => "env",
        Commands::Openclaw { .. } => "openclaw",
        Commands::Sessions { .. } => "sessions",
        Commands::Omo { .. } => "omo",
        Commands::OmoSlim { .. } => "omo-slim",
        Commands::Workspace { .. } => "workspace",
        Commands::Webdav { .. } => "webdav",
        Commands::Deeplink { .. } => "deeplink",
        Commands::Export { .. } => "export",
        Commands::Import { .. } => "import",
        Commands::ImportDeeplink { .. } => "import-deeplink",
        Commands::E2eSession => "e2e-session",
    }
}
