//! Prompt command handlers

use crate::cli::PromptCommands;
use crate::handlers::common::parse_app_type;
use crate::output::Printer;
use anyhow::Context;
use cc_switch_core::{AppState, Prompt};
use std::fs;
use std::path::Path;

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
    let app_type = parse_app_type(app)?;
    let file = file.ok_or_else(|| anyhow::anyhow!("Prompt add requires --file"))?;
    let content = read_text_file(file)?;
    let prompt_id = id
        .map(ToOwned::to_owned)
        .or_else(|| file_stem(file))
        .unwrap_or_else(|| format!("prompt-{}", chrono::Utc::now().timestamp()));

    if cc_switch_core::PromptService::get(state, app_type.clone(), &prompt_id)?.is_some() {
        anyhow::bail!(
            "Prompt already exists: {}. Use `prompt edit` instead.",
            prompt_id
        );
    }

    let now = now_seconds();
    let prompt = Prompt {
        id: prompt_id.clone(),
        name: prompt_id.clone(),
        content,
        description: Some(format!("Imported from {}", file)),
        enabled: false,
        created_at: Some(now),
        updated_at: Some(now),
    };

    cc_switch_core::PromptService::upsert_prompt(state, app_type, &prompt_id, prompt)?;
    printer.success(format!("✓ Added prompt '{}' for {}", prompt_id, app));
    Ok(())
}

async fn handle_edit(
    id: &str,
    app: &str,
    file: Option<&str>,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let file = file.ok_or_else(|| anyhow::anyhow!("Prompt edit requires --file"))?;
    let content = read_text_file(file)?;
    let mut prompt = cc_switch_core::PromptService::get(state, app_type.clone(), id)?
        .ok_or_else(|| anyhow::anyhow!("Prompt not found: {}", id))?;

    prompt.content = content;
    prompt.updated_at = Some(now_seconds());

    cc_switch_core::PromptService::upsert_prompt(state, app_type, id, prompt)?;
    printer.success(format!("✓ Updated prompt '{}' for {}", id, app));
    Ok(())
}

async fn handle_delete(
    id: &str,
    app: &str,
    yes: bool,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    if !yes {
        anyhow::bail!("Prompt delete is destructive. Re-run with --yes to confirm.");
    }

    let app_type = parse_app_type(app)?;
    cc_switch_core::PromptService::delete_prompt(state, app_type, id)?;
    printer.success(format!("✓ Deleted prompt '{}' for {}", id, app));
    Ok(())
}

async fn handle_enable(
    id: &str,
    app: &str,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    cc_switch_core::PromptService::enable(state, app_type, id)?;
    printer.success(format!("✓ Enabled prompt '{}' for {}", id, app));
    Ok(())
}

async fn handle_import(app: &str, state: &AppState, printer: &Printer) -> anyhow::Result<()> {
    let app_type = parse_app_type(app)?;
    let count = cc_switch_core::PromptService::import_from_files(state, app_type)?;
    printer.success(format!("✓ Imported {} prompts for {}", count, app));
    Ok(())
}

fn read_text_file(path: &str) -> anyhow::Result<String> {
    fs::read_to_string(path).with_context(|| format!("Failed to read file: {}", path))
}

fn file_stem(path: &str) -> Option<String> {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .map(|value| value.to_string())
}

fn now_seconds() -> i64 {
    chrono::Utc::now().timestamp()
}
