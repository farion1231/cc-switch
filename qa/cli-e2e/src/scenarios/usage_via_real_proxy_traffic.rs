use anyhow::Result;
use reqwest::StatusCode;
use serde_json::json;
use tokio::time::{sleep, Duration};

use crate::asserts::{ensure, stdout_json};
use crate::mock::{Family, MockServer, Profile};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{finalize, free_port};

const NAME: &str = "usage_via_real_proxy_traffic";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description:
            "real proxy traffic hits mock upstream and usage summary/logs/export reflect it",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let mock = MockServer::start().await?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        let provider_fixture = sandbox.fixture_path("providers/claude-settings.json");
        sandbox
            .run_ok(&vec![
                "provider".to_string(),
                "add".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--from-json".to_string(),
                provider_fixture.display().to_string(),
                "--name".to_string(),
                "Proxy Usage".to_string(),
                "--base-url".to_string(),
                mock.anthropic_base(),
                "--api-key".to_string(),
                "proxy-usage-key".to_string(),
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

        mock.set_profile(Family::Anthropic, Profile::OkJson).await;
        let client = reqwest::Client::new();
        let success = client
            .post(format!("http://127.0.0.1:{port}/v1/messages"))
            .header("content-type", "application/json")
            .header("x-api-key", "client-key")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": "claude-sonnet-4-5",
                "max_tokens": 64,
                "messages": [{ "role": "user", "content": "hello from e2e" }]
            }))
            .send()
            .await?;
        ensure(
            success.status() == StatusCode::OK,
            "proxy request should succeed",
        )?;

        mock.set_profile(Family::Anthropic, Profile::ServerError500)
            .await;
        let failure = client
            .post(format!("http://127.0.0.1:{port}/v1/messages"))
            .header("content-type", "application/json")
            .header("x-api-key", "client-key")
            .header("anthropic-version", "2023-06-01")
            .json(&json!({
                "model": "claude-sonnet-4-5",
                "max_tokens": 32,
                "messages": [{ "role": "user", "content": "please fail" }]
            }))
            .send()
            .await?;
        ensure(
            failure.status() == StatusCode::INTERNAL_SERVER_ERROR,
            "proxy should surface upstream 500",
        )?;

        sleep(Duration::from_millis(400)).await;

        session
            .run_ok(&vec!["proxy".to_string(), "stop".to_string()])
            .await?;
        session.close().await?;

        sleep(Duration::from_millis(250)).await;

        let summary = sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "summary".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--days".to_string(),
                "30".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let summary_json = stdout_json(&summary)?;
        ensure(
            summary_json["totalRequests"].as_u64().unwrap_or_default() >= 2,
            "usage summary did not capture proxy traffic",
        )?;
        ensure(
            summary_json["totalTokens"].as_u64().unwrap_or_default() > 0,
            "usage summary totalTokens should be populated",
        )?;

        let logs = sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "logs".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let logs_json = stdout_json(&logs)?;
        ensure(
            logs_json.as_array().is_some_and(|items| items.len() >= 2),
            "usage logs did not include proxy requests",
        )?;

        let trends = sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "trends".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let trends_json = stdout_json(&trends)?;
        ensure(
            trends_json.as_array().is_some_and(|items| !items.is_empty()),
            "usage trends should contain at least one point",
        )?;

        let provider_stats = sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "provider-stats".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let provider_stats_json = stdout_json(&provider_stats)?;
        ensure(
            provider_stats_json.as_array().is_some_and(|items| !items.is_empty()),
            "usage provider-stats should contain at least one row",
        )?;

        let model_stats = sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "model-stats".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let model_stats_json = stdout_json(&model_stats)?;
        ensure(
            model_stats_json.as_array().is_some_and(|items| !items.is_empty()),
            "usage model-stats should contain at least one row",
        )?;

        sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "model-pricing".to_string(),
                "update".to_string(),
                "e2e-custom-model".to_string(),
                "--display-name".to_string(),
                "E2E Custom Model".to_string(),
                "--input-cost".to_string(),
                "1.11".to_string(),
                "--output-cost".to_string(),
                "2.22".to_string(),
                "--cache-read-cost".to_string(),
                "0.11".to_string(),
                "--cache-creation-cost".to_string(),
                "0.22".to_string(),
            ])
            .await?;

        let pricing_list = sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "model-pricing".to_string(),
                "list".to_string(),
                "--format".to_string(),
                "json".to_string(),
            ])
            .await?;
        let pricing_json = stdout_json(&pricing_list)?;
        ensure(
            pricing_json.as_array().is_some_and(|items| items.iter().any(|item| {
                item["modelId"] == "e2e-custom-model"
            })),
            "usage model-pricing list should include the newly upserted row",
        )?;

        sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "model-pricing".to_string(),
                "delete".to_string(),
                "e2e-custom-model".to_string(),
            ])
            .await?;

        let export_path = sandbox.work_path("usage.csv");
        sandbox
            .run_ok(&vec![
                "usage".to_string(),
                "export".to_string(),
                "--app".to_string(),
                "claude".to_string(),
                "--output".to_string(),
                export_path.display().to_string(),
            ])
            .await?;
        ensure(export_path.exists(), "usage export CSV was not created")?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), Some(&mock)).await?;
    mock.shutdown().await;
    result
}
