//! Prompt command handlers

use crate::cli::PromptCommands;
use crate::output::Printer;
use cc_switch_core::AppState;

pub async fn handle(
    cmd: PromptCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        PromptCommands::List { app } => handle_list(&app, state, printer).await,
        PromptCommands::Show { id, app } => handle_show(&id, &app, state, printer).await,
        PromptCommands::Add { app, id, file } => {
            handle_add(&app, id.as_deref(), file.as_deref(), state, printer).await
        }
        PromptCommands::Edit { id, app, file } => {
            handle_edit(&id, &app, file.as_deref(), state, printer).await
        }
        PromptCommands::Delete { id, app, yes } => {
            handle_delete(&id, &app, yes, state, printer).await
        }
        PromptCommands::Enable { id, app } => handle_enable(&id, &app, state, printer).await,
        PromptCommands::Import { app } => handle_import(&app, state, printer).await,
    }
}

async fn handle_list(app: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let prompts = cc_switch_core::PromptService::list(state, app_type)?;
    printer.print_prompts(&prompts)?;
    Ok(())
}

async fn handle_show(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let prompts = cc_switch_core::PromptService::list(state, app_type)?;
    let prompt = prompts
        .get(id)
        .ok_or_else(|| anyhow::anyhow!("Prompt not found: {}", id))?;
    printer.print_prompt_detail(prompt)?;
    Ok(())
}

async fn handle_add(
    app: &str,
    id: Option<&str>,
    file: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement prompt add")
}

async fn handle_edit(
    id: &str,
    app: &str,
    file: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement prompt edit")
}

async fn handle_delete(
    id: &str,
    app: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    todo!("Implement prompt delete")
}

async fn handle_enable(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    cc_switch_core::PromptService::enable(state, app_type, id)?;
    println!("✓ Enabled prompt '{}' for {}", id, app);
    Ok(())
}

async fn handle_import(app: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let count = cc_switch_core::PromptService::import_from_files(state, app_type)?;
    println!("✓ Imported {} prompts for {}", count, app);
    Ok(())
}

fn parse_app_type(s: &str) -> anyhow::Result<cc_switch_core::AppType> {
    s.parse().map_err(|_| {
        anyhow::anyhow!(
            "Invalid app type: {}. Valid values: claude, codex, gemini, opencode",
            s
        )
    })
}
