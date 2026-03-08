//! Environment conflict command handlers

use crate::cli::EnvCommands;
use crate::output::Printer;
use anyhow::Context;
use serde_json::json;

pub async fn handle(cmd: EnvCommands, printer: &Printer) -> anyhow::Result<()> {
    match cmd {
        EnvCommands::Check { app } => handle_check(&app, printer).await,
        EnvCommands::Delete {
            app,
            include_system,
            yes,
        } => handle_delete(&app, include_system, yes, printer).await,
        EnvCommands::Restore { backup_path } => handle_restore(&backup_path, printer).await,
    }
}

async fn handle_check(app: &str, printer: &Printer) -> anyhow::Result<()> {
    validate_env_app(app)?;
    let conflicts = cc_switch_core::services::env_checker::check_env_conflicts(app)
        .map_err(anyhow::Error::msg)?;
    printer.print_value(&conflicts)
}

async fn handle_delete(
    app: &str,
    include_system: bool,
    yes: bool,
    printer: &Printer,
) -> anyhow::Result<()> {
    validate_env_app(app)?;
    if !yes {
        anyhow::bail!("Environment cleanup is destructive. Re-run with --yes to confirm.");
    }

    let conflicts = cc_switch_core::services::env_checker::check_env_conflicts(app)
        .map_err(anyhow::Error::msg)?;
    let selected: Vec<_> = if include_system {
        #[cfg(not(target_os = "windows"))]
        {
            if conflicts.iter().any(|conflict| conflict.source_type == "system") {
                anyhow::bail!(
                    "System environment conflicts cannot be safely restored on this platform. Re-run without --include-system."
                );
            }
        }
        conflicts
    } else {
        conflicts
            .into_iter()
            .filter(|conflict| conflict.source_type == "file")
            .collect()
    };

    if selected.is_empty() {
        printer.warn("No matching environment conflicts were found.");
        return printer.print_value(&json!({
            "app": app,
            "deleted": 0,
            "backupPath": serde_json::Value::Null,
        }));
    }

    let backup =
        cc_switch_core::services::env_manager::delete_env_vars(selected).map_err(anyhow::Error::msg)?;
    let deleted = backup.conflicts.len();
    printer.print_value(&json!({
        "app": app,
        "deleted": deleted,
        "backupPath": backup.backup_path,
        "timestamp": backup.timestamp,
        "conflicts": backup.conflicts,
    }))
}

async fn handle_restore(backup_path: &str, printer: &Printer) -> anyhow::Result<()> {
    cc_switch_core::services::env_manager::restore_from_backup(backup_path.to_string())
        .map_err(anyhow::Error::msg)
        .with_context(|| format!("failed to restore environment backup: {backup_path}"))?;
    printer.print_value(&json!({
        "backupPath": backup_path,
        "restored": true,
    }))
}

fn validate_env_app(app: &str) -> anyhow::Result<()> {
    match app.to_ascii_lowercase().as_str() {
        "claude" | "codex" | "gemini" => Ok(()),
        other => anyhow::bail!(
            "Environment conflict commands currently support claude, codex, and gemini. Unsupported app: {other}"
        ),
    }
}
