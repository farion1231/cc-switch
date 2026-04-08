use anyhow::Result;
use base64::prelude::*;

use crate::asserts::{ensure, read_json, stdout_json};
use crate::runner::{HarnessEnv, Scenario};
use crate::sandbox::Sandbox;
use crate::scenarios::util::{args, finalize};

const NAME: &str = "import_export_deeplink";

pub fn scenario() -> Scenario {
    Scenario {
        name: NAME,
        description: "export, import --merge and import-deeplink round-trip key config domains",
        run: |env| Box::pin(run(env)),
    }
}

async fn run(env: HarnessEnv) -> Result<()> {
    let mut sandbox = Sandbox::new(&env, NAME)?;
    let result = async {
        sandbox.stage_home_fixture("live-config/base")?;

        sandbox
            .run_ok(&args(&["config", "set", "preferredTerminal", "wezterm"]))
            .await?;

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
                "Export Alpha".to_string(),
                "--base-url".to_string(),
                "https://export.alpha.mock/v1".to_string(),
                "--api-key".to_string(),
                "export-alpha-key".to_string(),
            ])
            .await?;

        let prompt_fixture = sandbox.fixture_path("prompts/primary.md");
        sandbox
            .run_ok(&vec![
                "prompt".to_string(),
                "add".to_string(),
                "--app".to_string(),
                "codex".to_string(),
                "--id".to_string(),
                "export-guide".to_string(),
                "--file".to_string(),
                prompt_fixture.display().to_string(),
            ])
            .await?;

        let export_path = sandbox.work_path("export.json");
        sandbox
            .run_ok(&vec![
                "export".to_string(),
                "--output".to_string(),
                export_path.display().to_string(),
            ])
            .await?;
        let export_json = read_json(&export_path)?;
        ensure(
            export_json["providers"]["claude"]
                .get("export-alpha")
                .is_some(),
            "export file missing provider",
        )?;
        ensure(
            export_json["prompts"]["codex"]
                .get("export-guide")
                .is_some(),
            "export file missing prompt",
        )?;

        let merge_fixture = sandbox.fixture_path("import/merge.json");
        sandbox
            .run_ok(&vec![
                "import".to_string(),
                "--input".to_string(),
                merge_fixture.display().to_string(),
                "--merge".to_string(),
            ])
            .await?;

        let imported_provider = sandbox
            .run_ok(&args(&[
                "provider",
                "show",
                "merge-provider",
                "--app",
                "codex",
                "--format",
                "json",
            ]))
            .await?;
        ensure(
            stdout_json(&imported_provider)?["id"] == "merge-provider",
            "merge import did not create codex provider",
        )?;

        let imported_prompt = sandbox
            .run_ok(&args(&[
                "prompt",
                "show",
                "merged-guide",
                "--app",
                "claude",
                "--format",
                "json",
            ]))
            .await?;
        ensure(
            stdout_json(&imported_prompt)?["id"] == "merged-guide",
            "merge import did not create claude prompt",
        )?;

        let config_json = r#"{"env":{"ANTHROPIC_AUTH_TOKEN":"sk-ant-xxx","ANTHROPIC_BASE_URL":"https://api.anthropic.com/v1","ANTHROPIC_MODEL":"claude-sonnet-4.5"}}"#;
        let config_b64 = BASE64_STANDARD
            .encode(config_json.as_bytes())
            .replace('+', "%2B")
            .replace('/', "%2F")
            .replace('=', "%3D");
        let preview_url = format!(
            "ccswitch://v1/import?resource=provider&app=claude&name=Preview%20Provider&config={config_b64}&configFormat=json"
        );

        let parsed_output = sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "deeplink".to_string(),
                "parse".to_string(),
                preview_url.clone(),
            ])
            .await?;
        ensure(
            stdout_json(&parsed_output)?["name"] == "Preview Provider",
            "deeplink parse did not expose the parsed request",
        )?;

        let merged_output = sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "deeplink".to_string(),
                "merge".to_string(),
                preview_url.clone(),
            ])
            .await?;
        let merged_json = stdout_json(&merged_output)?;
        ensure(
            merged_json["apiKey"] == "sk-ant-xxx"
                && merged_json["endpoint"] == "https://api.anthropic.com/v1",
            "deeplink merge did not fill config-derived provider fields",
        )?;

        let preview_output = sandbox
            .run_ok(&vec![
                "--format".to_string(),
                "json".to_string(),
                "deeplink".to_string(),
                "preview".to_string(),
                preview_url,
            ])
            .await?;
        ensure(
            stdout_json(&preview_output)?["merged"]["model"] == "claude-sonnet-4.5",
            "deeplink preview did not expose the merged request view",
        )?;

        let deeplink_url = std::fs::read_to_string(sandbox.fixture_path("deeplink/provider.url"))?;
        sandbox
            .run_ok(&vec![
                "import-deeplink".to_string(),
                deeplink_url.trim().to_string(),
            ])
            .await?;
        let claude_providers = sandbox
            .run_ok(&args(&[
                "provider", "list", "--app", "claude", "--format", "json",
            ]))
            .await?;
        let providers_json = stdout_json(&claude_providers)?;
        ensure(
            providers_json.as_object().is_some_and(|providers| {
                providers
                    .values()
                    .any(|item| item["name"] == "DeepLink Provider")
            }),
            "deeplink import did not create provider with expected name",
        )?;

        Ok(())
    }
    .await;

    finalize(&sandbox, result.is_ok(), None).await?;
    result
}
