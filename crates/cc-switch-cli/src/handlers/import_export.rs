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
    cc_switch_core::ProviderService::sync_current_to_live(state)?;
    printer.success(format!("✓ Imported configuration from {}", input));
    Ok(())
}

pub async fn handle_deeplink(url: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let result = cc_switch_core::DeeplinkService::import(url, state)?;
    printer.success(format!("✓ Imported {} from deeplink", result.item_type));
    if !result.warnings.is_empty() {
        for warning in &result.warnings {
            printer.warn(format!("  Warning: {}", warning));
        }
    }
    Ok(())
}

pub async fn handle_deeplink_parse(url: &str, printer: &Printer) -> anyhow::Result<()> {
    let parsed = cc_switch_core::parse_deeplink_url(url)?;
    printer.print_value(&parsed)?;
    Ok(())
}

pub async fn handle_deeplink_merge(url: &str, printer: &Printer) -> anyhow::Result<()> {
    let parsed = cc_switch_core::parse_deeplink_url(url)?;
    let merged = cc_switch_core::parse_and_merge_config(&parsed)?;
    printer.print_value(&merged)?;
    Ok(())
}

pub async fn handle_deeplink_preview(url: &str, printer: &Printer) -> anyhow::Result<()> {
    let parsed = cc_switch_core::parse_deeplink_url(url)?;
    let merged = cc_switch_core::parse_and_merge_config(&parsed)?;
    printer.print_value(&serde_json::json!({
        "parsed": parsed,
        "merged": merged,
    }))?;
    Ok(())
}
