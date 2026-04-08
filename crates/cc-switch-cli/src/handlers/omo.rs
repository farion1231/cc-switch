//! OMO / OMO Slim command handlers

use serde_json::json;

use crate::cli::OmoCommands;
use crate::output::Printer;
use cc_switch_core::{AppState, OmoService, OmoVariant, SLIM, STANDARD};

pub async fn handle_standard(
    cmd: OmoCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    handle_variant(cmd, state, printer, &STANDARD).await
}

pub async fn handle_slim(
    cmd: OmoCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    handle_variant(cmd, state, printer, &SLIM).await
}

async fn handle_variant(
    cmd: OmoCommands,
    state: &AppState,
    printer: &Printer,
    variant: &OmoVariant,
) -> anyhow::Result<()> {
    match cmd {
        OmoCommands::ReadLocal => {
            let local = OmoService::read_local_file(variant)?;
            printer.print_value(&local)?;
        }
        OmoCommands::ImportLocal => {
            let provider = OmoService::import_from_local(state, variant)?;
            printer.print_provider_detail(&provider)?;
        }
        OmoCommands::Current => {
            let current = OmoService::get_current_provider_id(state, variant)?;
            printer.print_value(&json!({
                "variant": variant.category,
                "providerId": current,
            }))?;
        }
        OmoCommands::DisableCurrent => {
            OmoService::disable_current(state, variant)?;
            printer.print_value(&json!({
                "variant": variant.category,
                "disabled": true,
            }))?;
        }
    }

    Ok(())
}
