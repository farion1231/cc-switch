use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{finalize, free_port};

const NAME: &str = "proxy_failover_runtime";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description:
            "proxy failover queue order, missing-provider and unsupported-app runtime checks",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        let fixture = sandbox.fixture_path("providers/claude-settings.json");
        for (name, base_url, api_key) in [
            (
                "Claude Alpha",
                "https://alpha.failover.mock/v1",
                "alpha-failover-key",
            ),
            (
                "Claude Beta",
                "https://beta.failover.mock/v1",
                "beta-failover-key",
            ),
        ] {
            sandbox
                .run_ok(&vec![
                    "provider".to_string(),
                    "add".to_string(),
                    "--app".to_string(),
                    "claude".to_string(),
                    "--from-json".to_string(),
                    fixture.display().to_string(),
                    "--name".to_string(),
                    name.to_string(),
                    "--base-url".to_string(),
                    base_url.to_string(),
                    "--api-key".to_string(),
                    api_key.to_string(),
                ])
                .await?;
        }

        let unsupported = sandbox
            .run(&vec![
                "proxy".to_string(),
                "failover".to_string(),
                "queue".to_string(),
                "--app".to_string(),
                "opencode".to_string(),
            ])
            .await?;
        ensure(
            !unsupported.success(),
            "opencode proxy failover should be rejected",
        )?;

        let missing = sandbox
            .run(&vec![
                "proxy".to_string(),
                "failover".to_string(),
                "add".to_string(),
                "missing-provider".to_string(),
                "--app".to_string(),
                "claude".to_string(),
            ])
            .await?;
        ensure(
            !missing.success(),
            "missing provider should not be added to failover queue",
        )?;

        sandbox
            .run_ok(&vec![
                "proxy".to_string(),
                "failover".to_string(),
                "add".to_string(),
                "claude-alpha".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--priority".to_string(),
                "1".to_string(),
            ])
            .await?;
        sandbox
            .run_ok(&vec![
                "proxy".to_string(),
                "failover".to_string(),
                "add".to_string(),
                "claude-beta".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--priority".to_string(),
                "0".to_string(),
            ])
            .await?;

        let queue = sandbox
            .run_ok(&vec![
                "proxy".to_string(),
                "failover".to_string(),
                "queue".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let queue_json = stdout_json(&queue)?;
        ensure(
            queue_json.as_array().is_some_and(|items| {
                items
                    .first()
                    .is_some_and(|item| item["providerId"] == "claude-beta")
            }),
            "failover queue order did not honor priority",
        )?;

        let port = free_port()?;
        let mut session = sandbox.start_session().await?;
        session
            .run_ok(&vec![
                "proxy".to_string(),
                "start".to_string(),
                "--port".to_string(),
                port.to_string(),
                "--host".to_string(),
                "127.0.0.1".to_string(),
            ])
            .await?;
        session
            .run_ok(&vec![
                "proxy".to_string(),
                "failover".to_string(),
                "switch".to_string(),
                "claude-beta".to_string(),
                "--app".to_string(),
                "claude".to_string(),
            ])
            .await?;
        let status = session
            .run_ok(&vec![
                "proxy".to_string(),
                "status".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        ensure(
            status.stdout.contains("claude-beta"),
            "runtime status did not expose switched active target",
        )?;
        session
            .run_ok(&vec!["proxy".to_string(), "stop".to_string()])
            .await?;
        session.close().await?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
