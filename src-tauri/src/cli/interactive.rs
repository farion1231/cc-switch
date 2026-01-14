//! Interactive CLI mode using dialoguer

use console::{style, Term};
use dialoguer::{theme::ColorfulTheme, Select};

use crate::app_config::AppType;
use crate::services::ProviderService;
use crate::store::AppState;

/// Run interactive CLI mode
///
/// Presents a menu-driven interface for selecting tools and switching providers.
pub fn run_interactive(state: &AppState, term: &Term) -> Result<(), String> {
    let _ = term.write_line(&format!(
        "\n{}",
        style("CC-Switch CLI Mode").bold().cyan()
    ));
    let _ = term.write_line(&format!(
        "{}\n",
        style("Use arrow keys to navigate, Enter to select, Esc to quit").dim()
    ));

    loop {
        // Tool selection
        let tool = match select_tool(term)? {
            Some(t) => t,
            None => {
                let _ = term.write_line(&format!("\n{}", style("Goodbye!").dim()));
                return Ok(());
            }
        };

        // Provider selection for the chosen tool
        match select_and_switch_provider(state, term, tool)? {
            ProviderAction::Switched => {
                // Continue to allow more operations
                continue;
            }
            ProviderAction::Back => {
                // Go back to tool selection
                continue;
            }
            ProviderAction::Quit => {
                let _ = term.write_line(&format!("\n{}", style("Goodbye!").dim()));
                return Ok(());
            }
        }
    }
}

enum ProviderAction {
    Switched,
    Back,
    Quit,
}

/// Display tool type selection menu
fn select_tool(term: &Term) -> Result<Option<AppType>, String> {
    let tools = vec!["Claude", "Codex", "Gemini", "← Exit"];

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt("Select tool type")
        .items(&tools)
        .default(0)
        .interact_on_opt(term)
        .map_err(|e| format!("Selection error: {e}"))?;

    match selection {
        Some(0) => Ok(Some(AppType::Claude)),
        Some(1) => Ok(Some(AppType::Codex)),
        Some(2) => Ok(Some(AppType::Gemini)),
        Some(3) | None => Ok(None), // Exit
        _ => Ok(None),
    }
}

/// Display provider selection menu and handle switching
fn select_and_switch_provider(
    state: &AppState,
    term: &Term,
    app_type: AppType,
) -> Result<ProviderAction, String> {
    let providers = ProviderService::list(state, app_type.clone())
        .map_err(|e| format!("Failed to list providers: {e}"))?;

    if providers.is_empty() {
        let _ = term.write_line(&format!(
            "\n{} No providers configured for {}.",
            style("ℹ").blue(),
            app_type.as_str()
        ));
        let _ = term.write_line(&format!(
            "{}",
            style("Please use the GUI to add providers first.").dim()
        ));
        let _ = term.write_line("");
        return Ok(ProviderAction::Back);
    }

    let current_id = ProviderService::current(state, app_type.clone()).ok();

    // Build display items with active indicator
    let mut items: Vec<String> = providers
        .iter()
        .map(|(id, p)| {
            let is_active = current_id.as_ref() == Some(id);
            if is_active {
                format!("● {} [Active]", p.name)
            } else {
                format!("○ {}", p.name)
            }
        })
        .collect();

    items.push("← Back".to_string());

    // Find default selection (current active provider)
    let default_idx = current_id
        .as_ref()
        .and_then(|id| providers.get_index_of(id))
        .unwrap_or(0);

    let selection = Select::with_theme(&ColorfulTheme::default())
        .with_prompt(format!("Select provider for {}", app_type.as_str()))
        .items(&items)
        .default(default_idx)
        .interact_on_opt(term)
        .map_err(|e| format!("Selection error: {e}"))?;

    match selection {
        Some(idx) if idx == items.len() - 1 => {
            // "Back" selected
            Ok(ProviderAction::Back)
        }
        Some(idx) => {
            // Provider selected - perform switch
            let (provider_id, provider) = providers
                .get_index(idx)
                .ok_or_else(|| "Invalid selection".to_string())?;

            // Check if already active
            if current_id.as_ref() == Some(provider_id) {
                let _ = term.write_line(&format!(
                    "\n{} \"{}\" is already active for {}.\n",
                    style("ℹ").blue(),
                    provider.name,
                    app_type.as_str()
                ));
                return Ok(ProviderAction::Switched);
            }

            // Perform switch
            ProviderService::switch(state, app_type.clone(), provider_id)
                .map_err(|e| format!("Failed to switch provider: {e}"))?;

            let _ = term.write_line(&format!(
                "\n{} Switched {} provider to \"{}\"\n",
                style("✓").green(),
                app_type.as_str(),
                style(&provider.name).bold()
            ));

            Ok(ProviderAction::Switched)
        }
        None => {
            // Esc pressed
            Ok(ProviderAction::Quit)
        }
    }
}
