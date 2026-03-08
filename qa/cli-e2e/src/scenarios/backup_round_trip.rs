use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "backup_round_trip";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "backup create/list/rename/delete/restore keeps database state recoverable",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox
            .run_ok(&args(&[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                "before-backup",
                "--base-url",
                "https://before.example.com",
                "--api-key",
                "sk-before",
            ]))
            .await?;

        let create_output = sandbox
            .run_ok(&args(&["--format", "json", "backup", "create"]))
            .await?;
        let created = stdout_json(&create_output)?;
        let filename = created["filename"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("backup create did not return filename"))?
            .to_string();
        ensure(
            sandbox.home_path(&format!(".cc-switch/backups/{filename}")).exists(),
            "backup file should exist after create",
        )?;

        let list_output = sandbox
            .run_ok(&args(&["--format", "json", "backup", "list"]))
            .await?;
        let listed = stdout_json(&list_output)?;
        ensure(
            listed.as_array().is_some_and(|items| {
                items.iter().any(|item| item["filename"] == filename)
            }),
            "backup list should include created backup",
        )?;

        let rename_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "backup",
                "rename",
                &filename,
                "phase-two-checkpoint",
            ]))
            .await?;
        let renamed = stdout_json(&rename_output)?;
        let renamed_filename = renamed["renamedTo"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("backup rename did not return renamedTo"))?
            .to_string();

        sandbox
            .run_ok(&args(&[
                "provider",
                "add",
                "--app",
                "claude",
                "--name",
                "after-backup",
                "--base-url",
                "https://after.example.com",
                "--api-key",
                "sk-after",
            ]))
            .await?;

        let restore_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "backup",
                "restore",
                &renamed_filename,
                "--yes",
            ]))
            .await?;
        let restored = stdout_json(&restore_output)?;
        ensure(
            restored["filename"] == renamed_filename,
            "backup restore should echo the restored filename",
        )?;
        ensure(
            restored.get("safetyBackupId").and_then(serde_json::Value::as_str).is_some(),
            "backup restore should return safety backup id",
        )?;

        let provider_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "list",
                "--app",
                "claude",
            ]))
            .await?;
        let providers = stdout_json(&provider_output)?;
        ensure(
            providers.as_object().is_some_and(|items| {
                items.len() == 1
                    && items
                        .get("before-backup")
                        .and_then(|provider| provider.get("name"))
                        == Some(&serde_json::Value::String("before-backup".to_string()))
            }),
            "backup restore should roll database state back to the checkpoint",
        )?;

        sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "backup",
                "delete",
                &renamed_filename,
                "--yes",
            ]))
            .await?;
        ensure(
            !sandbox
                .home_path(&format!(".cc-switch/backups/{renamed_filename}"))
                .exists(),
            "backup delete should remove the renamed backup file",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
