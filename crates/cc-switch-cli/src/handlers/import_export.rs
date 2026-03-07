//! Import/Export command handlers

use crate::output::Printer;
use cc_switch_core::AppState;

pub async fn handle_export(
    output: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let data = cc_switch_core::ConfigService::export_all(&state.db)?;
    let json = serde_json::to_string_pretty(&data)?;
    std::fs::write(output, json)?;
    printer.success(format!("✓ Exported configuration to {}", output));
    Ok(())
}

pub async fn handle_import(
    input: &str,
    merge: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let json = std::fs::read_to_string(input)?;
    let data: serde_json::Value = serde_json::from_str(&json)?;
    cc_switch_core::ConfigService::import_all(&state.db, &data, merge)?;
    printer.success(format!("✓ Imported configuration from {}", input));
    Ok(())
}

pub async fn handle_deeplink(url: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let result = cc_switch_core::DeeplinkService::import(url, &state.db)?;
    printer.success(format!("✓ Imported {} from deeplink", result.item_type));
    if !result.warnings.is_empty() {
        for warning in &result.warnings {
            printer.warn(format!("  Warning: {}", warning));
        }
    }
    Ok(())
}
