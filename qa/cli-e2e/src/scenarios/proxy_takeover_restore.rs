use anyhow::Result;

use crate::asserts::{assert_contains, ensure, read_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{finalize, free_port};

const NAME: &str = "proxy_takeover_restore";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "proxy start/takeover/switch/stop restores live config from updated backup",
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
                "https://alpha.takeover.mock/v1",
                "alpha-takeover-key",
            ),
            (
                "Claude Beta",
                "https://beta.takeover.mock/v1",
                "beta-takeover-key",
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
        sandbox
            .run_ok(&vec![
                "provider".to_string(),
                "switch".to_string(),
                "claude-alpha".to_string(),
                "--app".to_string(),
                "claude".to_string(),
            ])
            .await?;

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
                "takeover".to_string(),
                "enable".to_string(),
                "--app".to_string(),
                "claude".to_string(),
            ])
            .await?;

        let takeover_live = read_json(&sandbox.home_path(".claude/settings.json"))?;
        ensure(
            takeover_live["env"]["ANTHROPIC_BASE_URL"] == format!("http://127.0.0.1:{port}"),
            "takeover did not rewrite Claude base URL to proxy",
        )?;
        ensure(
            takeover_live["env"]["ANTHROPIC_AUTH_TOKEN"] == "PROXY_MANAGED",
            "takeover did not replace Claude token with placeholder",
        )?;

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
        assert_contains(
            &status.stdout,
            "claude-beta",
            "proxy status active target after switch",
        )?;

        session
            .run_ok(&vec!["proxy".to_string(), "stop".to_string()])
            .await?;
        session.close().await?;

        let restored = read_json(&sandbox.home_path(".claude/settings.json"))?;
        ensure(
            restored["env"]["ANTHROPIC_BASE_URL"] == "https://beta.takeover.mock/v1",
            "proxy stop did not restore beta provider base URL",
        )?;
        ensure(
            restored["env"]["ANTHROPIC_AUTH_TOKEN"] == "beta-takeover-key",
            "proxy stop did not restore beta provider token",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
