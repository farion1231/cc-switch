mod asserts;
mod mock;
mod runner;
mod sandbox;
mod scenarios;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "cc-switch-cli-e2e")]
#[command(about = "Independent CLI sandbox E2E harness for cc-switch")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    List,
    Run { scenario: String },
    RunAll,
    Doctor,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let env = runner::HarnessEnv::detect()?;

    match cli.command {
        Command::List => runner::list_scenarios(&env),
        Command::Run { scenario } => runner::run_named(env, &scenario).await,
        Command::RunAll => runner::run_all(env).await,
        Command::Doctor => runner::doctor(&env),
    }
}
