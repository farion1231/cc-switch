use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "skill_local_lifecycle";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "local skill import/enable/disable/uninstall works with staged SSOT skill",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;
        sandbox.stage_fixture_to_path(
            "skills/ssot/demo-skill",
            &sandbox.home_path(".cc-switch/skills/demo-skill"),
        )?;

        let import_fixture = sandbox.fixture_path("import/skills.json");
        sandbox
            .run_ok(&vec![
                "import".to_string(),
                "--input".to_string(),
                import_fixture.display().to_string(),
                "--merge".to_string(),
            ])
            .await?;

        let list_output = sandbox
            .run_ok(&args(&["skill", "list", "--format", "json"]))
            .await?;
        let list_json = stdout_json(&list_output)?;
        ensure(
            list_json
                .as_array()
                .is_some_and(|items| items.iter().any(|item| item["id"] == "local:demo-skill")),
            "skill import did not create local:demo-skill",
        )?;

        sandbox
            .run_ok(&args(&[
                "skill",
                "enable",
                "local:demo-skill",
                "--app",
                "claude",
            ]))
            .await?;
        ensure(
            sandbox
                .home_path(".claude/skills/demo-skill/SKILL.md")
                .exists(),
            "skill enable did not sync to Claude skills dir",
        )?;

        sandbox
            .run_ok(&args(&[
                "skill",
                "disable",
                "local:demo-skill",
                "--app",
                "claude",
            ]))
            .await?;
        ensure(
            !sandbox.home_path(".claude/skills/demo-skill").exists(),
            "skill disable did not remove from Claude skills dir",
        )?;

        let uninstall_without_yes = sandbox
            .run(&args(&["skill", "uninstall", "local:demo-skill"]))
            .await?;
        ensure(
            !uninstall_without_yes.success(),
            "skill uninstall without --yes should fail",
        )?;

        sandbox
            .run_ok(&args(&["skill", "uninstall", "local:demo-skill", "--yes"]))
            .await?;
        ensure(
            !sandbox.home_path(".cc-switch/skills/demo-skill").exists(),
            "skill uninstall did not remove SSOT skill directory",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
