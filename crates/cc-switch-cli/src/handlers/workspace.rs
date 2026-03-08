//! Workspace command handlers

use std::fs;

use anyhow::Context;
use serde_json::json;

use crate::cli::{WorkspaceCommands, WorkspaceMemoryCommands};
use crate::output::Printer;
use cc_switch_core::WorkspaceService;

pub async fn handle(cmd: WorkspaceCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        WorkspaceCommands::Read { filename } => handle_read(&filename, printer),
        WorkspaceCommands::Write {
            filename,
            file,
            value,
        } => handle_write(&filename, file.as_deref(), value.as_deref(), printer),
        WorkspaceCommands::Memory { cmd } => handle_memory(cmd, printer),
    }
}

fn handle_read(filename: &str, printer: &Printer) -> anyhow::Result<()> {
    let content = WorkspaceService::read_workspace_file(filename)?;
    printer.print_value(&json!({
        "filename": filename,
        "content": content,
    }))?;
    Ok(())
}

fn handle_write(
    filename: &str,
    file: Option<&str>,
    value: Option<&str>,
    printer: &Printer,
) -> anyhow::Result<()> {
    let content = resolve_content("workspace write", file, value)?;
    WorkspaceService::write_workspace_file(filename, &content)?;
    printer.print_value(&json!({
        "filename": filename,
        "written": true,
    }))?;
    Ok(())
}

fn handle_memory(cmd: WorkspaceMemoryCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        WorkspaceMemoryCommands::List => {
            let files = WorkspaceService::list_daily_memory_files()?;
            printer.print_value(&files)?;
        }
        WorkspaceMemoryCommands::Read { filename } => {
            let content = WorkspaceService::read_daily_memory_file(&filename)?;
            printer.print_value(&json!({
                "filename": filename,
                "content": content,
            }))?;
        }
        WorkspaceMemoryCommands::Write {
            filename,
            file,
            value,
        } => {
            let content = resolve_content("workspace memory write", file.as_deref(), value.as_deref())?;
            WorkspaceService::write_daily_memory_file(&filename, &content)?;
            printer.print_value(&json!({
                "filename": filename,
                "written": true,
            }))?;
        }
        WorkspaceMemoryCommands::Search { query } => {
            let results = WorkspaceService::search_daily_memory_files(&query)?;
            printer.print_value(&results)?;
        }
        WorkspaceMemoryCommands::Delete { filename } => {
            WorkspaceService::delete_daily_memory_file(&filename)?;
            printer.print_value(&json!({
                "filename": filename,
                "deleted": true,
            }))?;
        }
    }

    Ok(())
}

fn resolve_content(label: &str, file: Option<&str>, value: Option<&str>) -> anyhow::Result<String> {
    match (file, value) {
        (Some(path), None) => {
            fs::read_to_string(path).with_context(|| format!("{label} failed to read file: {path}"))
        }
        (None, Some(value)) => Ok(value.to_string()),
        _ => anyhow::bail!("{label} requires either --file or --value"),
    }
}
