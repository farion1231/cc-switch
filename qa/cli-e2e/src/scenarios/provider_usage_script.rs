use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::mock::{Family, MockServer, Profile};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "provider_usage_script";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "provider usage-script save/show/test/query/clear works in sandbox",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let mock = MockServer::start().await?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;
        mock.set_profile(Family::Anthropic, Profile::OkJson).await;

        let provider_file = sandbox.work_path("provider.json");
        sandbox.write_text(
            &provider_file,
            r#"{"env":{"ANTHROPIC_BASE_URL":"https://unused.mock","ANTHROPIC_AUTH_TOKEN":"usage-script-key"}}"#,
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
                "Usage Script Alpha".to_string(),
                "--base-url".to_string(),
                "https://unused.mock".to_string(),
                "--api-key".to_string(),
                "usage-script-key".to_string(),
            ])
            .await?;

        let script_file = sandbox.work_path("usage-script.json");
        let script_json = serde_json::json!({
            "enabled": true,
            "language": "javascript",
            "timeout": 5,
            "templateType": "custom",
            "code": format!(
                "({{ request: {{ url: \"{}/v1/messages\", method: \"POST\", headers: {{ \"content-type\": \"application/json\" }}, body: \"{{}}\" }}, extractor: function(response) {{ return {{ isValid: true, remaining: response.usage.input_tokens, unit: \"tokens\" }}; }} }})",
                mock.anthropic_base()
            ),
        });
        sandbox.write_text(
            &script_file,
            &serde_json::to_string_pretty(&script_json)?,
        )?;

        sandbox
            .run_ok(&vec![
                "provider".to_string(),
                "usage-script".to_string(),
                "save".to_string(),
                "usage-script-alpha".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--file".to_string(),
                script_file.display().to_string(),
            ])
            .await?;

        let show_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "usage-script",
                "show",
                "usage-script-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let show_json = stdout_json(&show_output)?;
        ensure(
            show_json["usageScript"]["enabled"] == true,
            "saved usage script should be visible in show output",
        )?;

        let test_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "usage-script",
                "test",
                "usage-script-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let test_json = stdout_json(&test_output)?;
        ensure(
            test_json["success"] == true && test_json["data"][0]["remaining"] == 12.0,
            "usage-script test should parse mock response",
        )?;

        let query_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "usage-script",
                "query",
                "usage-script-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let query_json = stdout_json(&query_output)?;
        ensure(
            query_json["success"] == true && query_json["data"][0]["remaining"] == 12.0,
            "usage-script query should parse mock response",
        )?;

        let requests: serde_json::Value = serde_json::from_str(&mock.requests_json().await?)?;
        ensure(
            requests.as_array().is_some_and(|items| items.len() >= 2),
            "mock server should receive at least test and query requests",
        )?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "usage-script",
                "save",
                "usage-script-alpha",
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
                "usage-script",
                "show",
                "usage-script-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let cleared_json = stdout_json(&cleared_output)?;
        ensure(
            cleared_json
                .get("usageScript")
                .is_some_and(serde_json::Value::is_null),
            "cleared usage script should become null",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), Some(&mock)).await?;
    mock.shutdown().await;
    result
}
