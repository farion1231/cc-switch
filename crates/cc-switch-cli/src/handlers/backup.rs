//! Backup command handlers

use serde_json::json;

use crate::cli::BackupCommands;
use crate::output::Printer;
use cc_switch_core::{AppState, Database};

pub async fn handle(
    cmd: BackupCommands,
    state: &AppState,
    printer: &Printer,
) -> anyhow::Result<()> {
    match cmd {
        BackupCommands::Create => {
            let filename = state
                .db
                .create_backup()?
                .and_then(|path| path.file_name().map(|name| name.to_string_lossy().to_string()))
                .ok_or_else(|| anyhow::anyhow!("Database file not found, backup skipped"))?;
            printer.print_value(&json!({ "filename": filename }))?;
        }
        BackupCommands::List => {
            let backups = Database::list_backups()?;
            printer.print_value(&backups)?;
        }
        BackupCommands::Restore { filename, yes } => {
            if !yes {
                anyhow::bail!("Backup restore is destructive. Re-run with --yes to confirm.");
            }

            let safety_backup_id = state.db.restore_from_backup(&filename)?;
            printer.print_value(&json!({
                "filename": filename,
                "safetyBackupId": safety_backup_id,
            }))?;
        }
        BackupCommands::Rename { filename, new_name } => {
            let renamed = Database::rename_backup(&filename, &new_name)?;
            printer.print_value(&json!({
                "filename": filename,
                "renamedTo": renamed,
            }))?;
        }
        BackupCommands::Delete { filename, yes } => {
            if !yes {
                anyhow::bail!("Backup delete is destructive. Re-run with --yes to confirm.");
            }

            Database::delete_backup(&filename)?;
            printer.print_value(&json!({
                "filename": filename,
                "deleted": true,
            }))?;
        }
    }

    Ok(())
}
