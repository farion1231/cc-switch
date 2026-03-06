//! Usage command handlers

use crate::cli::UsageCommands;
use crate::output::Printer;
use cc_switch_core::{AppState, UsageStatsService};

pub async fn handle(cmd: UsageCommands, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        UsageCommands::Summary { app, days } => handle_summary(&app, days, state, printer).await,
        UsageCommands::Logs { app, from, to } => {
            handle_logs(&app, from.as_deref(), to.as_deref(), state, printer).await
        }
        UsageCommands::Export { output, app } => handle_export(&output, &app, state, printer).await,
    }
}

async fn handle_summary(
    app: &str,
    days: u32,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let summary = UsageStatsService::get_summary(&state.db, app, days)?;
    printer.print_usage_summary(&summary)?;
    Ok(())
}

async fn handle_logs(
    app: &str,
    from: Option<&str>,
    to: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let logs = UsageStatsService::get_logs(&state.db, app, from, to)?;
    printer.print_usage_logs(&logs)?;
    Ok(())
}

async fn handle_export(
    output: &str,
    app: &str,
    state: &AppState,
    _printer: &Printer,
) -> anyhow::Result<()> {
    let path = UsageStatsService::export_csv(&state.db, app, output)?;
    println!("✓ Exported usage data to {}", path);
    Ok(())
}
