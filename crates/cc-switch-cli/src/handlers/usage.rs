//! Usage command handlers

use crate::cli::{UsageCommands, UsageModelPricingCommands, UsageProviderLimitsCommands};
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use anyhow::Context;
use cc_switch_core::{AppState, ModelPricingInfo, UsageService};
use chrono::{NaiveDate, TimeZone, Utc};

pub async fn handle(cmd: UsageCommands, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        UsageCommands::Summary { app, days } => handle_summary(&app, days, state, printer).await,
        UsageCommands::Logs { app, from, to } => {
            handle_logs(&app, from.as_deref(), to.as_deref(), state, printer).await
        }
        UsageCommands::Export { output, app } => handle_export(&output, &app, state, printer).await,
        UsageCommands::Trends { from, to } => {
            handle_trends(from.as_deref(), to.as_deref(), state, printer).await
        }
        UsageCommands::ProviderStats => handle_provider_stats(state, printer).await,
        UsageCommands::ModelStats => handle_model_stats(state, printer).await,
        UsageCommands::RequestDetail { request_id } => {
            handle_request_detail(&request_id, state, printer).await
        }
        UsageCommands::ModelPricing { cmd } => handle_model_pricing(cmd, state, printer).await,
        UsageCommands::ProviderLimits { cmd } => handle_provider_limits(cmd, state, printer).await,
    }
}

async fn handle_summary(
    app: &str,
    days: Option<u32>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app = parse_app_type(app)?;
    let summary = match days {
        Some(days) => UsageService::get_summary(&state.db, app.as_str(), days)?,
        None => UsageService::get_summary_all(&state.db, app.as_str())?,
    };
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

async fn handle_trends(
    from: Option<&str>,
    to: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let start_ts = parse_date_start(from)?;
    let end_ts = parse_date_end(to)?;
    let trends = UsageService::get_trends(&state.db, start_ts, end_ts)?;
    printer.print_value(&trends)?;
    Ok(())
}

async fn handle_provider_stats(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let stats = UsageService::get_provider_stats(&state.db)?;
    printer.print_value(&stats)?;
    Ok(())
}

async fn handle_model_stats(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let stats = UsageService::get_model_stats(&state.db)?;
    printer.print_value(&stats)?;
    Ok(())
}

async fn handle_request_detail(
    request_id: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let detail = UsageService::get_request_detail(&state.db, request_id)?
        .ok_or_else(|| anyhow::anyhow!("Request not found: {}", request_id))?;
    printer.print_value(&detail)?;
    Ok(())
}

async fn handle_model_pricing(
    cmd: UsageModelPricingCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        UsageModelPricingCommands::List => {
            let pricing = UsageService::get_model_pricing(&state.db)?;
            printer.print_value(&pricing)?;
        }
        UsageModelPricingCommands::Update {
            model_id,
            display_name,
            input_cost,
            output_cost,
            cache_read_cost,
            cache_creation_cost,
        } => {
            UsageService::update_model_pricing(
                &state.db,
                ModelPricingInfo {
                    model_id: model_id.clone(),
                    display_name,
                    input_cost_per_million: input_cost,
                    output_cost_per_million: output_cost,
                    cache_read_cost_per_million: cache_read_cost,
                    cache_creation_cost_per_million: cache_creation_cost,
                },
            )?;
            printer.success(format!("✓ Updated model pricing '{}'", model_id));
        }
        UsageModelPricingCommands::Delete { model_id } => {
            UsageService::delete_model_pricing(&state.db, &model_id)?;
            printer.success(format!("✓ Deleted model pricing '{}'", model_id));
        }
    }

    Ok(())
}

async fn handle_provider_limits(
    cmd: UsageProviderLimitsCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        UsageProviderLimitsCommands::Check { provider_id, app } => {
            let app = parse_app_type(&app)?;
            let status =
                UsageService::check_provider_limits(&state.db, &provider_id, app.as_str())?;
            printer.print_value(&status)?;
        }
    }

    Ok(())
}

fn parse_date_start(date: Option<&str>) -> anyhow::Result<Option<i64>> {
    date.map(parse_date_utc_start).transpose()
}

fn parse_date_end(date: Option<&str>) -> anyhow::Result<Option<i64>> {
    date.map(parse_date_utc_end).transpose()
}

fn parse_date_utc_start(date: &str) -> anyhow::Result<i64> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .with_context(|| format!("Invalid date format: {date}. Expected YYYY-MM-DD"))?;
    Ok(Utc
        .from_utc_datetime(
            &date
                .and_hms_milli_opt(0, 0, 0, 0)
                .ok_or_else(|| anyhow::anyhow!("Invalid date: {}", date))?,
        )
        .timestamp_millis())
}

fn parse_date_utc_end(date: &str) -> anyhow::Result<i64> {
    let date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .with_context(|| format!("Invalid date format: {date}. Expected YYYY-MM-DD"))?;
    Ok(Utc
        .from_utc_datetime(
            &date
                .and_hms_milli_opt(23, 59, 59, 999)
                .ok_or_else(|| anyhow::anyhow!("Invalid date: {}", date))?,
        )
        .timestamp_millis())
}
