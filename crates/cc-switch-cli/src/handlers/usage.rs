//! Usage command handlers

use crate::cli::UsageCommands;
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use cc_switch_core::{AppState, UsageService};

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
    let app = parse_app_type(app)?;
    let summary = UsageService::get_summary(&state.db, app.as_str(), days)?;
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
    let app = parse_app_type(app)?;
    let logs = UsageService::get_request_logs(&state.db, app.as_str(), from, to)?;
    printer.print_usage_logs(&logs)?;
    Ok(())
}

async fn handle_export(
    output: &str,
    app: &str,
    state: &AppState,
    _printer: &Printer,
) -> anyhow::Result<()> {
    let app = parse_app_type(app)?;
    let path = UsageService::export_csv(&state.db, app.as_str(), output)?;
    _printer.success(format!("✓ Exported usage data to {}", path));
    Ok(())
}
