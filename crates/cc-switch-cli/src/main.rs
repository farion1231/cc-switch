//! CC-Switch CLI Entry Point

use cc_switch_core::{AppState, Database};
use clap::Parser;

mod cli;
mod e2e_session;
mod handlers;
mod output;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();

    let db = Database::new()?;
    let state = AppState::new(db);

    if matches!(args.command, cli::Commands::E2eSession) {
        return e2e_session::run(state).await;
    }

    handlers::dispatch(args, state).await
}
