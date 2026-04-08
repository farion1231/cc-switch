use anyhow::Result;

use crate::asserts::{ensure, read_text, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "prompt_live_sync";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "prompt add/edit/enable/delete/import syncs live prompt files",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        let primary = sandbox.fixture_path("prompts/primary.md");
        let secondary = sandbox.fixture_path("prompts/secondary.md");
        let updated = sandbox.fixture_path("prompts/updated.md");
        let codex_import = sandbox.fixture_path("prompts/codex-import.md");

        sandbox
            .run_ok(&vec![
                "prompt".to_string(),
                "add".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--id".to_string(),
                "prompt-a".to_string(),
                "--file".to_string(),
                primary.display().to_string(),
            ])
            .await?;
        sandbox
            .run_ok(&vec![
                "prompt".to_string(),
                "add".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--id".to_string(),
                "prompt-b".to_string(),
                "--file".to_string(),
                secondary.display().to_string(),
            ])
            .await?;

        sandbox
            .run_ok(&args(&["prompt", "enable", "prompt-a", "--app", "claude"]))
            .await?;
        ensure(
            read_text(&sandbox.home_path(".claude/CLAUDE.md"))? == read_text(&primary)?,
            "enabled prompt did not sync to CLAUDE.md",
        )?;
        let live_content = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "prompt",
                "current-live-file-content",
                "--app",
                "claude",
            ]))
            .await?;
        ensure(
            stdout_json(&live_content)?["content"] == read_text(&primary)?,
            "prompt current-live-file-content did not read the live prompt file",
        )?;

        sandbox
            .run_ok(&vec![
                "prompt".to_string(),
                "edit".to_string(),
                "prompt-a".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--file".to_string(),
                updated.display().to_string(),
            ])
            .await?;
        ensure(
            read_text(&sandbox.home_path(".claude/CLAUDE.md"))? == read_text(&updated)?,
            "edited prompt did not update CLAUDE.md",
        )?;

        sandbox
            .run_ok(&args(&["prompt", "enable", "prompt-b", "--app", "claude"]))
            .await?;
        ensure(
            read_text(&sandbox.home_path(".claude/CLAUDE.md"))? == read_text(&secondary)?,
            "switching enabled prompt did not update live file",
        )?;

        let delete_without_yes = sandbox
            .run(&args(&["prompt", "delete", "prompt-a", "--app", "claude"]))
            .await?;
        ensure(
            !delete_without_yes.success(),
            "prompt delete without --yes should fail",
        )?;

        sandbox
            .run_ok(&args(&[
                "prompt", "delete", "prompt-a", "--app", "claude", "--yes",
            ]))
            .await?;
        let list_output = sandbox
            .run_ok(&args(&[
                "prompt", "list", "--app", "claude", "--format", "json",
            ]))
            .await?;
        let list_json = stdout_json(&list_output)?;
        ensure(
            list_json.get("prompt-a").is_none(),
            "prompt-a should be deleted",
        )?;

        sandbox.write_home_text(".codex/AGENTS.md", &read_text(&codex_import)?)?;
        sandbox
            .run_ok(&args(&["prompt", "import", "--app", "codex"]))
            .await?;
        let codex_prompts = sandbox
            .run_ok(&args(&[
                "prompt", "list", "--app", "codex", "--format", "json",
            ]))
            .await?;
        let codex_json = stdout_json(&codex_prompts)?;
        ensure(
            codex_json
                .as_object()
                .is_some_and(|value| !value.is_empty()),
            "prompt import did not create codex prompt",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
