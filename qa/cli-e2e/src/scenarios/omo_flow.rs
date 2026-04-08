use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "omo_flow";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "omo and omo-slim read/import/current/disable flows work against local jsonc files",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.write_home_text(
            ".config/opencode/oh-my-opencode.jsonc",
            r#"{
  // comment
  "agents": { "writer": { "prompt": "hi" } },
  "categories": { "default": ["writer"] },
  "theme": "default"
}"#,
        )?;
        sandbox.write_home_text(
            ".config/opencode/oh-my-opencode-slim.jsonc",
            r#"{
  "agents": { "reviewer": { "prompt": "ship it" } },
  "theme": "slim"
}"#,
        )?;

        let read_omo = sandbox
            .run_ok(&args(&["--format", "json", "omo", "read-local"]))
            .await?;
        let read_omo_json = stdout_json(&read_omo)?;
        ensure(
            read_omo_json.get("agents").is_some() && read_omo_json.get("categories").is_some(),
            "omo read-local should expose agents and categories",
        )?;

        let import_omo = sandbox
            .run_ok(&args(&["--format", "json", "omo", "import-local"]))
            .await?;
        let import_omo_json = stdout_json(&import_omo)?;
        let imported_omo_id = import_omo_json["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("omo import-local did not return provider id"))?
            .to_string();

        let current_omo = sandbox
            .run_ok(&args(&["--format", "json", "omo", "current"]))
            .await?;
        let current_omo_json = stdout_json(&current_omo)?;
        ensure(
            current_omo_json["providerId"] == imported_omo_id,
            "omo current should return the imported provider id",
        )?;

        sandbox
            .run_ok(&args(&["--format", "json", "omo", "disable-current"]))
            .await?;
        ensure(
            !sandbox
                .home_path(".config/opencode/oh-my-opencode.jsonc")
                .exists(),
            "omo disable-current should remove the generated config file",
        )?;

        let read_omo_slim = sandbox
            .run_ok(&args(&["--format", "json", "omo-slim", "read-local"]))
            .await?;
        let read_omo_slim_json = stdout_json(&read_omo_slim)?;
        ensure(
            read_omo_slim_json.get("agents").is_some()
                && (read_omo_slim_json.get("categories").is_none()
                    || read_omo_slim_json
                        .get("categories")
                        .is_some_and(serde_json::Value::is_null)),
            "omo-slim read-local should expose agents without categories",
        )?;

        let import_omo_slim = sandbox
            .run_ok(&args(&["--format", "json", "omo-slim", "import-local"]))
            .await?;
        let import_omo_slim_json = stdout_json(&import_omo_slim)?;
        let imported_omo_slim_id = import_omo_slim_json["id"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("omo-slim import-local did not return provider id"))?
            .to_string();

        let current_omo_slim = sandbox
            .run_ok(&args(&["--format", "json", "omo-slim", "current"]))
            .await?;
        let current_omo_slim_json = stdout_json(&current_omo_slim)?;
        ensure(
            current_omo_slim_json["providerId"] == imported_omo_slim_id,
            "omo-slim current should return the imported provider id",
        )?;

        sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "omo-slim",
                "disable-current",
            ]))
            .await?;
        ensure(
            !sandbox
                .home_path(".config/opencode/oh-my-opencode-slim.jsonc")
                .exists(),
            "omo-slim disable-current should remove the generated config file",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
