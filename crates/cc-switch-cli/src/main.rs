//! CC-Switch CLI Entry Point

use cc_switch_core::{AppState, Database};
use clap::Parser;

mod cli;
mod handlers;
mod output;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = cli::Cli::parse();

    let db = Database::new()?;
    let state = AppState::new(db);

    handlers::dispatch(args, state).await
}
