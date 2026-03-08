use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::args;

const NAME: &str = "session_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "sessions list/messages/resume-command reads discovered session files",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        let session_path = sandbox.home_path(".claude/projects/demo-project/session-1.jsonl");
        sandbox.write_text(
            &session_path,
            concat!(
                "{\"sessionId\":\"session-1\",\"cwd\":\"/work/demo-project\",\"timestamp\":\"2026-03-08T10:00:00Z\",\"isMeta\":true}\n",
                "{\"message\":{\"role\":\"user\",\"content\":\"hello from claude\"},\"timestamp\":\"2026-03-08T10:01:00Z\"}\n",
                "{\"message\":{\"role\":\"assistant\",\"content\":\"done\"},\"timestamp\":\"2026-03-08T10:02:00Z\"}\n"
            ),
        )?;

        let list_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "sessions",
                "list",
                "--provider",
                "claude",
                "--query",
                "demo-project",
            ]))
            .await?;
        let listed = stdout_json(&list_output)?;
        ensure(
            listed.as_array().is_some_and(|items| {
                items.len() == 1 && items[0]["resumeCommand"] == "claude --resume session-1"
            }),
            "sessions list should return the discovered claude session",
        )?;

        let messages_output = sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "sessions".to_string(),
                "messages".to_string(),
                "--provider".to_string(),
                "claude".to_string(),
                "--source-path".to_string(),
                session_path.display().to_string(),
            ])
            .await?;
        let messages = stdout_json(&messages_output)?;
        ensure(
            messages.as_array().is_some_and(|items| {
                items.len() == 2 && items[0]["content"] == "hello from claude"
            }),
            "sessions messages should return the conversation payload",
        )?;

        let resume_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "sessions",
                "resume-command",
                "session-1",
                "--provider",
                "claude",
            ]))
            .await?;
        let resume = stdout_json(&resume_output)?;
        ensure(
            resume["resumeCommand"] == "claude --resume session-1",
            "sessions resume-command should expose the generated resume command",
        )?;

        Ok(())
    }
    .await;

    sandbox.finalize(result.is_ok(), None)?;
    result
}
