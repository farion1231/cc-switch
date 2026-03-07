use anyhow::Result;

use crate::asserts::{assert_contains, ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "config_smoke";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "config set/get/show/path and quiet/verbose smoke checks",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox
            .run_ok(&args(&["config", "set", "preferredTerminal", "ghostty"]))
            .await?;

        let get_output = sandbox
            .run_ok(&args(&[
                "config",
                "get",
                "preferredTerminal",
                "--format",
                "json",
            ]))
            .await?;
        let get_json = stdout_json(&get_output)?;
        ensure(
            get_json["preferredTerminal"] == "ghostty",
            "preferredTerminal setting did not persist",
        )?;

        let show_output = sandbox
            .run_ok(&args(&["config", "show", "--format", "json"]))
            .await?;
        let show_json = stdout_json(&show_output)?;
        ensure(
            show_json["preferredTerminal"] == "ghostty",
            "config show did not include preferredTerminal",
        )?;

        let path_output = sandbox
            .run_ok(&args(&["config", "path", "--format", "json"]))
            .await?;
        let path_json = stdout_json(&path_output)?;
        ensure(
            path_json["configDir"]
                .as_str()
                .is_some_and(|value| value.ends_with("/.cc-switch")),
            "config path did not point to fake HOME",
        )?;

        let quiet_output = sandbox
            .run_ok(&args(&["--quiet", "config", "show", "--format", "json"]))
            .await?;
        ensure(
            quiet_output.stdout.trim().is_empty(),
            "--quiet should suppress stdout",
        )?;

        let verbose_output = sandbox
            .run_ok(&args(&["--verbose", "config", "show", "--format", "json"]))
            .await?;
        assert_contains(
            &verbose_output.stderr,
            "Executing config command",
            "--verbose stderr",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
