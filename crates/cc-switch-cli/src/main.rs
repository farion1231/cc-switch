//! CC-Switch CLI Entry Point

use cc_switch_core::{AppError, AppState, Database};
use clap::Parser;

mod cli;
mod e2e_session;
mod handlers;
mod output;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();

    if matches!(args.command, cli::Commands::E2eSession) {
        let db = Database::new()?;
        let state = AppState::new(db);
        state.run_startup_maintenance();
        return e2e_session::run(state).await;
    }

    if !handlers::command_needs_state(&args.command) {
        return handlers::dispatch_stateless(args).await;
    }

    let state = initialize_state(&args.command)?;

    handlers::dispatch(args, state).await
}

fn initialize_state(command: &cli::Commands) -> anyhow::Result<AppState> {
    match Database::new() {
        Ok(db) => {
            let state = AppState::new(db);
            state.run_startup_maintenance();
            Ok(state)
        }
        Err(err) if can_fallback_to_read_only(command, &err) => {
            eprintln!(
                "warning: state initialization fell back to read-only database mode for {}: {}",
                command_label(command),
                err
            );
            Ok(AppState::new(Database::open_read_only()?))
        }
        Err(err) => Err(err.into()),
    }
}

fn can_fallback_to_read_only(command: &cli::Commands, err: &AppError) -> bool {
    matches!(command, cli::Commands::Doctor { .. })
        && matches!(
            err,
            AppError::Database(message)
                if {
                    let lower = message.to_ascii_lowercase();
                    lower.contains("readonly") || lower.contains("read-only")
                }
        )
}

fn command_label(command: &cli::Commands) -> &'static str {
    match command {
        cli::Commands::Doctor { .. } => "doctor",
        cli::Commands::Provider { .. } => "provider",
        cli::Commands::Mcp { .. } => "mcp",
        cli::Commands::Proxy { .. } => "proxy",
        cli::Commands::Prompt { .. } => "prompt",
        cli::Commands::Skill { .. } => "skill",
        cli::Commands::Settings { .. } => "settings",
        cli::Commands::Config { .. } => "config",
        cli::Commands::Usage { .. } => "usage",
        cli::Commands::Backup { .. } => "backup",
        cli::Commands::Omo { .. } => "omo",
        cli::Commands::OmoSlim { .. } => "omo-slim",
        cli::Commands::Webdav { .. } => "webdav",
        cli::Commands::Export { .. } => "export",
        cli::Commands::Import { .. } => "import",
        cli::Commands::ImportDeeplink { .. } => "import-deeplink",
        cli::Commands::Completions { .. } => "completions",
        cli::Commands::Install { .. } => "install",
        cli::Commands::AutoLaunch { .. } => "auto-launch",
        cli::Commands::PortableMode => "portable-mode",
        cli::Commands::ToolVersions { .. } => "tool-versions",
        cli::Commands::About => "about",
        cli::Commands::Update { .. } => "update",
        cli::Commands::ReleaseNotes { .. } => "release-notes",
        cli::Commands::Env { .. } => "env",
        cli::Commands::Openclaw { .. } => "openclaw",
        cli::Commands::Sessions { .. } => "sessions",
        cli::Commands::Workspace { .. } => "workspace",
        cli::Commands::Deeplink { .. } => "deeplink",
        cli::Commands::E2eSession => "e2e-session",
    }
}
