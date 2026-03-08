use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::mock::{Family, MockServer, Profile};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "provider_stream_check";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "provider stream-check config/run/run-all works in sandbox",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let mock = MockServer::start().await?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;
        mock.set_profile(Family::Anthropic, Profile::OkJson).await;

        let healthy_provider = sandbox.work_path("healthy-provider.json");
        sandbox.write_text(
            &healthy_provider,
            &format!(
                r#"{{"env":{{"ANTHROPIC_BASE_URL":"{}","ANTHROPIC_AUTH_TOKEN":"stream-check-ok"}}}}"#,
                mock.anthropic_base()
            ),
        )?;

        let broken_provider = sandbox.work_path("broken-provider.json");
        sandbox.write_text(
            &broken_provider,
            r#"{"env":{"ANTHROPIC_BASE_URL":"http://127.0.0.1:9","ANTHROPIC_AUTH_TOKEN":"stream-check-bad"}}"#,
        )?;

        for (name, file, base_url, api_key) in [
            (
                "Healthy Stream",
                healthy_provider.display().to_string(),
                mock.anthropic_base(),
                "stream-check-ok".to_string(),
            ),
            (
                "Broken Stream",
                broken_provider.display().to_string(),
                "http://127.0.0.1:9".to_string(),
                "stream-check-bad".to_string(),
            ),
        ] {
            sandbox
                .run_ok(&vec![
                    "provider".to_string(),
                    "add".to_string(),
                    "--app".to_string(),
                    "claude".to_string(),
                    "--from-json".to_string(),
                    file,
                    "--name".to_string(),
                    name.to_string(),
                    "--base-url".to_string(),
                    base_url,
                    "--api-key".to_string(),
                    api_key,
                ])
                .await?;
        }

        let config_file = sandbox.work_path("stream-check.json");
        sandbox.write_text(
            &config_file,
            r#"{
  "timeoutSecs": 2,
  "maxRetries": 0,
  "degradedThresholdMs": 9999,
  "claudeModel": "claude-haiku-4-5-20251001",
  "codexModel": "gpt-5.1-codex@low",
  "geminiModel": "gemini-3-pro-preview",
  "testPrompt": "hello stream check"
}"#,
        )?;

        sandbox
            .run_ok(&vec![
                "provider".to_string(),
                "stream-check".to_string(),
                "config".to_string(),
                "set".to_string(),
                "--file".to_string(),
                config_file.display().to_string(),
            ])
            .await?;

        let config_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "stream-check",
                "config",
                "get",
            ]))
            .await?;
        let config_json = stdout_json(&config_output)?;
        ensure(
            config_json["timeoutSecs"] == 2 && config_json["maxRetries"] == 0,
            "stream-check config should round-trip",
        )?;

        let single_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "stream-check",
                "run",
                "healthy-stream",
                "--app",
                "claude",
            ]))
            .await?;
        let single_json = stdout_json(&single_output)?;
        ensure(
            single_json["success"] == true && single_json["status"] == "operational",
            "single stream-check should succeed for healthy provider",
        )?;

        let all_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "stream-check",
                "run-all",
                "--app",
                "claude",
            ]))
            .await?;
        let all_json = stdout_json(&all_output)?;
        ensure(
            all_json.as_array().map(|items| items.len()) == Some(2),
            "run-all should return both providers",
        )?;
        ensure(
            all_json.as_array().is_some_and(|items| items.iter().any(|item| {
                item["providerId"] == "healthy-stream" && item["success"] == true
            })),
            "run-all should contain a healthy provider result",
        )?;
        ensure(
            all_json.as_array().is_some_and(|items| items.iter().any(|item| {
                item["providerId"] == "broken-stream" && item["success"] == false
            })),
            "run-all should contain a failed provider result",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), Some(&mock)).await?;
    mock.shutdown().await;
    result
}
