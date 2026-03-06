//! Skill command handlers

use crate::cli::SkillCommands;
use crate::output::Printer;
use cc_switch_core::AppState;

pub async fn handle(cmd: SkillCommands, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        SkillCommands::List => handle_list(state, printer).await,
        SkillCommands::Search { keyword } => handle_search(&keyword, state, printer).await,
        SkillCommands::Install { id, app } => handle_install(&id, &app, state, printer).await,
        SkillCommands::Uninstall { id, yes } => handle_uninstall(&id, yes, state, printer).await,
        SkillCommands::Enable { id, app } => handle_toggle(&id, &app, true, state, printer).await,
        SkillCommands::Disable { id, app } => handle_toggle(&id, &app, false, state, printer).await,
    }
}

async fn handle_list(state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let skills = cc_switch_core::SkillService::get_all_installed(&state.db)?;
    printer.print_skills(&skills)?;
    Ok(())
}

async fn handle_search(keyword: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    todo!("Implement skill search")
}

async fn handle_install(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement skill install")
}

async fn handle_uninstall(
    id: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement skill uninstall")
}

async fn handle_toggle(
    id: &str,
    app: &str,
    enabled: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement skill toggle")
}

fn parse_app_type(s: &str) -> anyhow::Result<cc_switch_core::AppType> {
    s.parse().map_err(|_| {
        anyhow::anyhow!(
            "Invalid app type: {}. Valid values: claude, codex, gemini, opencode",
            s
        )
    })
}
