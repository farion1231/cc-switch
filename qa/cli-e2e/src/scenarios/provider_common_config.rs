use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "provider_common_config";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "provider common-config-snippet extract/get/set/clear works in sandbox",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        let provider_file = sandbox.work_path("provider.json");
        sandbox.write_text(
            &provider_file,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://extract.mock/v1","ANTHROPIC_AUTH_TOKEN":"snippet-key","HTTPS_PROXY":"http://127.0.0.1:8080"},"permissions":{"allow":["Bash"]}}"#,
        )?;

        sandbox
            .run_ok(&vec![
                "provider".to_string(),
                "add".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--from-json".to_string(),
                provider_file.display().to_string(),
                "--name".to_string(),
                "Snippet Alpha".to_string(),
                "--base-url".to_string(),
                "https://extract.mock/v1".to_string(),
                "--api-key".to_string(),
                "snippet-key".to_string(),
            ])
            .await?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "switch",
                "snippet-alpha",
                "--app",
                "claude",
            ]))
            .await?;

        let extract_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "common-config-snippet",
                "extract",
                "--app",
                "claude",
            ]))
            .await?;
        let extract_json = stdout_json(&extract_output)?;
        let snippet_text = extract_json
            .get("snippet")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let snippet_json: serde_json::Value =
            serde_json::from_str(snippet_text).expect("extract snippet should be valid json");
        ensure(
            snippet_json
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_BASE_URL"))
                .is_none(),
            "extract should strip ANTHROPIC_BASE_URL",
        )?;
        ensure(
            snippet_json
                .get("env")
                .and_then(|env| env.get("ANTHROPIC_AUTH_TOKEN"))
                .is_none(),
            "extract should strip ANTHROPIC_AUTH_TOKEN",
        )?;
        ensure(
            snippet_json["env"]["HTTPS_PROXY"] == "http://127.0.0.1:8080",
            "extract should keep shared proxy settings",
        )?;

        let snippet_file = sandbox.work_path("snippet.json");
        let saved_snippet =
            r#"{"env":{"HTTPS_PROXY":"http://10.0.0.2:8080"},"permissions":{"allow":["Read"]}}"#;
        sandbox.write_text(&snippet_file, saved_snippet)?;

        sandbox
            .run_ok(&vec![
                "provider".to_string(),
                "common-config-snippet".to_string(),
                "set".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--file".to_string(),
                snippet_file.display().to_string(),
            ])
            .await?;

        let get_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "common-config-snippet",
                "get",
                "--app",
                "claude",
            ]))
            .await?;
        let get_json = stdout_json(&get_output)?;
        ensure(
            get_json["snippet"] == saved_snippet,
            "saved common config snippet should round-trip",
        )?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "common-config-snippet",
                "set",
                "--app",
                "claude",
                "--clear",
            ]))
            .await?;

        let cleared_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "common-config-snippet",
                "get",
                "--app",
                "claude",
            ]))
            .await?;
        let cleared_json = stdout_json(&cleared_output)?;
        ensure(
            cleared_json.get("snippet").is_some_and(serde_json::Value::is_null),
            "cleared common config snippet should become null",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
