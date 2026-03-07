//! Config command handlers

use crate::cli::ConfigCommands;
use crate::output::Printer;
use cc_switch_core::AppState;

pub async fn handle(
    cmd: ConfigCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        ConfigCommands::Show => handle_show(state, printer).await,
        ConfigCommands::Get { key } => handle_get(&key, state, printer).await,
        ConfigCommands::Set { key, value } => handle_set(&key, &value, state, printer).await,
        ConfigCommands::Path => handle_path(state, printer).await,
    }
}

async fn handle_show(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let settings = cc_switch_core::ConfigService::get_settings(&state.db)?;
    printer.print_settings(&settings)?;
    Ok(())
}

async fn handle_get(key: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let value = cc_switch_core::ConfigService::get_setting(&state.db, key)?
        .ok_or_else(|| anyhow::anyhow!("Setting not found: {}", key))?;
    printer.print_value(&serde_json::json!({ key: value }))
}

async fn handle_set(
    key: &str,
    value: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    cc_switch_core::ConfigService::set_setting(&state.db, key, value)?;
    printer.success(format!("✓ Set {} = {}", key, value));
    Ok(())
}

async fn handle_path(_state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    printer.print_value(&serde_json::json!({
        "configDir": cc_switch_core::config::config_dir(),
        "database": cc_switch_core::config::database_path(),
        "settings": cc_switch_core::config::settings_path(),
    }))
}
