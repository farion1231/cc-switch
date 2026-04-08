use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "provider_endpoints";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "provider endpoint add/list/mark-used/speedtest/remove works in sandbox",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;
        let fixture_path = sandbox.fixture_path("providers/claude-settings.json");

        sandbox
            .run_ok(&vec![
                "provider".to_string(),
                "add".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--from-json".to_string(),
                fixture_path.display().to_string(),
                "--name".to_string(),
                "Endpoint Alpha".to_string(),
                "--base-url".to_string(),
                "http://127.0.0.1:9/v1".to_string(),
                "--api-key".to_string(),
                "endpoint-key".to_string(),
            ])
            .await?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "endpoint",
                "add",
                "endpoint-alpha",
                "--app",
                "claude",
                "--url",
                "not-a-url",
            ]))
            .await?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "endpoint",
                "add",
                "endpoint-alpha",
                "--app",
                "claude",
                "--url",
                "http://127.0.0.1:9/secondary/",
            ]))
            .await?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "endpoint",
                "mark-used",
                "endpoint-alpha",
                "--app",
                "claude",
                "--url",
                "http://127.0.0.1:9/secondary",
            ]))
            .await?;

        let list_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "endpoint",
                "list",
                "endpoint-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let list_json = stdout_json(&list_output)?;
        ensure(
            list_json.as_array().map(|items| items.len()) == Some(2),
            "provider endpoint list should contain two custom endpoints",
        )?;
        ensure(
            list_json.as_array().is_some_and(|items| items.iter().any(|item| {
                item.get("url") == Some(&serde_json::Value::String("http://127.0.0.1:9/secondary".to_string()))
                    && item
                        .get("lastUsed")
                        .and_then(serde_json::Value::as_i64)
                        .is_some()
            })),
            "provider endpoint mark-used should persist lastUsed",
        )?;

        let speedtest_output = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "endpoint",
                "speedtest",
                "endpoint-alpha",
                "--app",
                "claude",
                "--timeout",
                "2",
            ]))
            .await?;
        let speedtest_json = stdout_json(&speedtest_output)?;
        ensure(
            speedtest_json.as_array().is_some_and(|items| items.iter().any(|item| {
                item.get("url") == Some(&serde_json::Value::String("not-a-url".to_string()))
                    && item
                        .get("error")
                        .and_then(serde_json::Value::as_str)
                        .is_some_and(|text| text.starts_with("URL 无效"))
            })),
            "provider endpoint speedtest should report invalid custom endpoint",
        )?;

        sandbox
            .run_ok(&args(&[
                "provider",
                "endpoint",
                "remove",
                "endpoint-alpha",
                "--app",
                "claude",
                "--url",
                "not-a-url",
            ]))
            .await?;

        let list_after_remove = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "provider",
                "endpoint",
                "list",
                "endpoint-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let list_after_remove_json = stdout_json(&list_after_remove)?;
        ensure(
            list_after_remove_json.as_array().map(|items| items.len()) == Some(1),
            "provider endpoint remove should remove only the targeted entry",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
