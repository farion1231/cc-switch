use anyhow::Result;

use crate::asserts::{ensure, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "proxy_advanced_config";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description:
            "proxy global/app config, auto-failover, provider-health and pricing settings work in sandbox",
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
                "https://alpha.proxy-advanced.mock/v1",
                "alpha-advanced-key",
            ),
            (
                "Claude Beta",
                "https://beta.proxy-advanced.mock/v1",
                "beta-advanced-key",
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
            .run_ok(&args(&[
                "provider",
                "switch",
                "claude-alpha",
                "--app",
                "claude",
            ]))
            .await?;

        let global_set = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "global-config",
                "set",
                "--proxy-enabled",
                "true",
                "--host",
                "0.0.0.0",
                "--port",
                "18080",
                "--log-enabled",
                "false",
            ]))
            .await?;
        let global_set_json = stdout_json(&global_set)?;
        ensure(
            global_set_json["listenAddress"] == "0.0.0.0",
            "global-config set did not persist listenAddress",
        )?;
        ensure(
            global_set_json["listenPort"] == 18080,
            "global-config set did not persist listenPort",
        )?;

        let app_set = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "app-config",
                "set",
                "--app",
                "claude",
                "--enabled",
                "true",
                "--max-retries",
                "9",
                "--circuit-min-requests",
                "12",
            ]))
            .await?;
        let app_set_json = stdout_json(&app_set)?;
        ensure(
            app_set_json["maxRetries"] == 9,
            "app-config set did not persist maxRetries",
        )?;
        ensure(
            app_set_json["circuitMinRequests"] == 12,
            "app-config set did not persist circuitMinRequests",
        )?;

        let auto_failover = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "auto-failover",
                "enable",
                "--app",
                "claude",
            ]))
            .await?;
        let auto_failover_json = stdout_json(&auto_failover)?;
        ensure(
            auto_failover_json["activeProviderId"] == "claude-alpha",
            "auto-failover enable did not switch to current provider as P1",
        )?;

        let available = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "available-providers",
                "--app",
                "claude",
            ]))
            .await?;
        let available_json = stdout_json(&available)?;
        ensure(
            available_json.get("claude-alpha").is_none(),
            "current P1 provider should not remain in available-providers",
        )?;
        ensure(
            available_json.get("claude-beta").is_some(),
            "remaining provider should still appear in available-providers",
        )?;

        let multiplier_set = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "default-cost-multiplier",
                "set",
                "--app",
                "claude",
                "1.25",
            ]))
            .await?;
        let multiplier_set_json = stdout_json(&multiplier_set)?;
        ensure(
            multiplier_set_json["value"] == "1.25",
            "default-cost-multiplier set did not round-trip",
        )?;

        let pricing_set = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "pricing-model-source",
                "set",
                "--app",
                "claude",
                "request",
            ]))
            .await?;
        let pricing_set_json = stdout_json(&pricing_set)?;
        ensure(
            pricing_set_json["value"] == "request",
            "pricing-model-source set did not round-trip",
        )?;

        let health = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "provider-health",
                "claude-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let health_json = stdout_json(&health)?;
        ensure(
            health_json["is_healthy"] == true,
            "provider-health should default to healthy before runtime failures",
        )?;

        let stats = sandbox
            .run_ok(&args(&[
                "--format",
                "json",
                "proxy",
                "circuit",
                "stats",
                "claude-alpha",
                "--app",
                "claude",
            ]))
            .await?;
        let stats_json = stdout_json(&stats)?;
        ensure(
            stats_json.get("stats").is_some_and(serde_json::Value::is_null),
            "circuit stats should be null when the proxy is not running",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
