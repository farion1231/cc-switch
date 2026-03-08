use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "openclaw_config_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "openclaw env/tools/default-model/model-catalog/agents-defaults round-trip through CLI",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        let catalog_file = sandbox.work_path("model-catalog.json");
        sandbox.write_text(
            &catalog_file,
            r#"{
  "demo/gpt-5": {
    "alias": "GPT-5",
    "contextWindow": 200000
  }
}"#,
        )?;

        sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "openclaw",
                "env",
                "set",
                "--value",
                r#"{"OPENAI_API_KEY":"sk-openclaw","OPENCLAW_FEATURE":"enabled"}"#,
            ]))
            .await?;
        sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "openclaw",
                "tools",
                "set",
                "--value",
                r#"{"profile":"strict","allow":["read:*"],"deny":["write:*"]}"#,
            ]))
            .await?;
        sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "openclaw",
                "default-model",
                "set",
                "--value",
                r#"{"primary":"demo/gpt-5","fallbacks":["demo/gpt-4.1"]}"#,
            ]))
            .await?;
        sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "openclaw".to_string(),
                "model-catalog".to_string(),
                "set".to_string(),
                "--file".to_string(),
                catalog_file.display().to_string(),
            ])
            .await?;
        sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "openclaw",
                "agents-defaults",
                "set",
                "--value",
                r#"{
  "model": {
    "primary": "demo/gpt-5",
    "fallbacks": ["demo/gpt-4.1"]
  },
  "models": {
    "demo/gpt-5": {
      "alias": "GPT-5"
    }
  }
}"#,
            ]))
            .await?;

        let env_output = sandbox
            .run_ok(&args(&["--format", "json", "openclaw", "env", "get"]))
            .await?;
        let env_json = stdout_json(&env_output)?;
        ensure(
            env_json["OPENAI_API_KEY"] == "sk-openclaw",
            "openclaw env get should return the saved API key",
        )?;

        let tools_output = sandbox
            .run_ok(&args(&["--format", "json", "openclaw", "tools", "get"]))
            .await?;
        let tools_json = stdout_json(&tools_output)?;
        ensure(
            tools_json["profile"] == "strict",
            "openclaw tools get should return the saved profile",
        )?;

        let default_model_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "openclaw",
                "default-model",
                "get",
            ]))
            .await?;
        let default_model_json = stdout_json(&default_model_output)?;
        ensure(
            default_model_json["primary"] == "demo/gpt-5",
            "openclaw default-model get should return the saved primary model",
        )?;

        let model_catalog_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "openclaw",
                "model-catalog",
                "get",
            ]))
            .await?;
        let model_catalog_json = stdout_json(&model_catalog_output)?;
        ensure(
            model_catalog_json["demo/gpt-5"]["alias"] == "GPT-5",
            "openclaw model-catalog get should return the saved alias",
        )?;

        let agents_defaults_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "openclaw",
                "agents-defaults",
                "get",
            ]))
            .await?;
        let agents_defaults_json = stdout_json(&agents_defaults_output)?;
        ensure(
            agents_defaults_json["model"]["primary"] == "demo/gpt-5",
            "openclaw agents-defaults get should return the saved default model",
        )?;

        let openclaw_json = std::fs::read_to_string(sandbox.home_path(".openclaw/openclaw.json"))?;
        ensure(
            openclaw_json.contains("\"env\"")
                && openclaw_json.contains("\"tools\"")
                && openclaw_json.contains("\"agents\""),
            "openclaw live config should contain env, tools and agents sections",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
