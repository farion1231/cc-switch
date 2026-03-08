use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "env_conflict_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "env check/delete/restore manages shell-file conflicts through a backup file",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        let zshrc_path = sandbox.home_path(".zshrc");
        sandbox.write_text(
            &zshrc_path,
            "export ANTHROPIC_E2E_TOKEN=sk-env\nexport OTHER_VAR=ok\n",
        )?;

        let check_output = sandbox
            .run_ok(&args(&["--format", "json", "env", "check", "--app", "claude"]))
            .await?;
        let conflicts = stdout_json(&check_output)?;
        ensure(
            conflicts.as_array().is_some_and(|items| {
                items.iter().any(|item| {
                    item["varName"] == "ANTHROPIC_E2E_TOKEN" && item["sourceType"] == "file"
                })
            }),
            "env check should surface the shell-file conflict",
        )?;

        let delete_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "env",
                "delete",
                "--app",
                "claude",
                "--yes",
            ]))
            .await?;
        let deleted = stdout_json(&delete_output)?;
        let backup_path = deleted["backupPath"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("env delete did not return a backup path"))?
            .to_string();
        let after_delete = std::fs::read_to_string(&zshrc_path)?;
        ensure(
            !after_delete.contains("ANTHROPIC_E2E_TOKEN"),
            "env delete should remove the target export line",
        )?;

        let restore_output = sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "env".to_string(),
                "restore".to_string(),
                backup_path.clone(),
            ])
            .await?;
        let restored = stdout_json(&restore_output)?;
        ensure(
            restored["restored"] == true,
            "env restore should report success",
        )?;
        let after_restore = std::fs::read_to_string(&zshrc_path)?;
        ensure(
            after_restore.contains("export ANTHROPIC_E2E_TOKEN=sk-env"),
            "env restore should put the shell export line back",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
