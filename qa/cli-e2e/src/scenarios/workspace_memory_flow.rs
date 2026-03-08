use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "workspace_memory_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "workspace and daily memory commands read, write, search and delete content",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        let workspace_write = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "workspace",
                "write",
                "AGENTS.md",
                "--value",
                "sandbox workspace text",
            ]))
            .await?;
        let workspace_write_json = stdout_json(&workspace_write)?;
        ensure(
            workspace_write_json["written"] == true,
            "workspace write should report success",
        )?;

        let workspace_read = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "workspace",
                "read",
                "AGENTS.md",
            ]))
            .await?;
        let workspace_read_json = stdout_json(&workspace_read)?;
        ensure(
            workspace_read_json["content"] == "sandbox workspace text",
            "workspace read should return the saved content",
        )?;

        let memory_file = sandbox.work_path("2026-03-08.md");
        sandbox.write_text(&memory_file, "Stage two workspace notes\nProxy parity done\n")?;

        sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "workspace".to_string(),
                "memory".to_string(),
                "write".to_string(),
                "2026-03-08.md".to_string(),
                "--file".to_string(),
                memory_file.display().to_string(),
            ])
            .await?;

        let list_output = sandbox
            .run_ok(&args(&["--format", "json", "workspace", "memory", "list"]))
            .await?;
        let list_json = stdout_json(&list_output)?;
        ensure(
            list_json.as_array().is_some_and(|items| {
                items.iter().any(|item| item["filename"] == "2026-03-08.md")
            }),
            "memory list should contain the written file",
        )?;

        let search_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "workspace",
                "memory",
                "search",
                "proxy parity",
            ]))
            .await?;
        let search_json = stdout_json(&search_output)?;
        ensure(
            search_json.as_array().is_some_and(|items| {
                items
                    .first()
                    .and_then(|item| item.get("snippet"))
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(|snippet| snippet.contains("Proxy parity"))
            }),
            "memory search should return a matching snippet",
        )?;

        let read_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "workspace",
                "memory",
                "read",
                "2026-03-08.md",
            ]))
            .await?;
        let read_json = stdout_json(&read_output)?;
        ensure(
            read_json["content"] == "Stage two workspace notes\nProxy parity done\n",
            "memory read should return the saved content",
        )?;

        let delete_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "workspace",
                "memory",
                "delete",
                "2026-03-08.md",
            ]))
            .await?;
        let delete_json = stdout_json(&delete_output)?;
        ensure(
            delete_json["deleted"] == true,
            "memory delete should report success",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
